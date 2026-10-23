/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::object_kind;
use crate::catalog_rows::{int_at, query_rows, text_at};
use crate::column_type::{CatalogType, declared_type};
use oracle::{Connection, Row};
use relix_core::{ColumnInfo, ForeignKeyInfo, IndexInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;

const RELATION_TYPE_SQL: &str = "SELECT object_type FROM all_objects \
     WHERE owner = :1 AND object_name = :2 \
       AND object_type IN ('TABLE', 'VIEW', 'MATERIALIZED VIEW') \
     ORDER BY CASE object_type WHEN 'MATERIALIZED VIEW' THEN 0 ELSE 1 END";

const COLUMNS_SQL: &str = "SELECT c.column_name, c.data_type, c.data_length, c.char_length, \
       c.char_used, c.data_precision, c.data_scale, c.nullable, c.data_default, c.column_id, \
       c.identity_column, cc.comments \
     FROM all_tab_columns c \
     LEFT JOIN all_col_comments cc ON cc.owner = c.owner AND cc.table_name = c.table_name \
       AND cc.column_name = c.column_name \
     WHERE c.owner = :1 AND c.table_name = :2 \
     ORDER BY c.column_id";

const PRIMARY_KEY_SQL: &str = "SELECT cc.column_name, c.index_name \
     FROM all_constraints c \
     JOIN all_cons_columns cc ON cc.owner = c.owner AND cc.constraint_name = c.constraint_name \
     WHERE c.owner = :1 AND c.table_name = :2 AND c.constraint_type = 'P' \
     ORDER BY cc.position";

const INDEXES_SQL: &str = "SELECT i.index_name, i.uniqueness, ic.column_name \
     FROM all_indexes i \
     JOIN all_ind_columns ic ON ic.index_owner = i.owner AND ic.index_name = i.index_name \
     WHERE i.table_owner = :1 AND i.table_name = :2 \
     ORDER BY i.index_name, ic.column_position";

const FOREIGN_KEYS_SQL: &str = "SELECT c.constraint_name, cc.column_name, r.owner, \
       r.table_name, rc.column_name \
     FROM all_constraints c \
     JOIN all_cons_columns cc ON cc.owner = c.owner AND cc.constraint_name = c.constraint_name \
     JOIN all_constraints r ON r.owner = c.r_owner AND r.constraint_name = c.r_constraint_name \
     JOIN all_cons_columns rc ON rc.owner = r.owner AND rc.constraint_name = r.constraint_name \
       AND rc.position = cc.position \
     WHERE c.owner = :1 AND c.table_name = :2 AND c.constraint_type = 'R' \
     ORDER BY c.constraint_name, cc.position";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableStructure {
    pub kind: ObjectKind,
    pub columns: Vec<ColumnInfo>,
    pub primary_key: Vec<String>,
    pub indexes: Vec<IndexInfo>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
}

/// Reads columns, keys and indexes of a table, view or materialized view. `table` must
/// carry the owner as its schema.
pub fn table_structure(
    connection: &Connection,
    table: &TableRef,
) -> Result<TableStructure, DriverError> {
    let owner = table.schema.as_deref().unwrap_or_default();
    let name = table.name.as_str();
    let kind = query_rows(connection, RELATION_TYPE_SQL, &[&owner, &name])?
        .first()
        .and_then(|row| text_at(row, 0))
        .and_then(|object_type| object_kind(&object_type))
        .ok_or_else(|| DriverError::query(format!("table or view {table} does not exist")))?;
    let primary_rows = query_rows(connection, PRIMARY_KEY_SQL, &[&owner, &name])?;
    let primary_key: Vec<String> = primary_rows
        .iter()
        .filter_map(|row| text_at(row, 0))
        .collect();
    let primary_index = primary_rows.first().and_then(|row| text_at(row, 1));
    let columns = query_rows(connection, COLUMNS_SQL, &[&owner, &name])?
        .iter()
        .filter_map(|row| column_info(row, &primary_key))
        .collect();
    let index_rows = query_rows(connection, INDEXES_SQL, &[&owner, &name])?;
    let mut indexes = group_indexes(&index_rows, primary_index.as_deref());
    indexes.sort_by_key(|index| !index.is_primary);
    let key_rows = query_rows(connection, FOREIGN_KEYS_SQL, &[&owner, &name])?;
    Ok(TableStructure {
        kind,
        columns,
        primary_key,
        indexes,
        foreign_keys: group_foreign_keys(&key_rows, table.database.as_deref()),
    })
}

fn column_info(row: &Row, primary_key: &[String]) -> Option<ColumnInfo> {
    let name = text_at(row, 0)?;
    let catalog = CatalogType {
        data_type: text_at(row, 1)?,
        data_length: int_at(row, 2).unwrap_or_default(),
        char_length: int_at(row, 3).unwrap_or_default(),
        char_semantics: text_at(row, 4).as_deref() == Some("C"),
        precision: int_at(row, 5),
        scale: int_at(row, 6),
    };
    Some(ColumnInfo {
        data_type: declared_type(&catalog),
        nullable: text_at(row, 7).as_deref() != Some("N"),
        default_value: text_at(row, 8)
            .map(|default| default.trim().to_owned())
            .filter(|default| !default.is_empty()),
        ordinal: u32::try_from(int_at(row, 9).unwrap_or_default()).unwrap_or_default(),
        is_primary_key: primary_key.contains(&name),
        is_auto_increment: text_at(row, 10).as_deref() == Some("YES"),
        comment: text_at(row, 11).filter(|comment| !comment.is_empty()),
        name,
    })
}

fn group_indexes(rows: &[Row], primary_index: Option<&str>) -> Vec<IndexInfo> {
    let mut indexes: Vec<IndexInfo> = Vec::new();
    for row in rows {
        let (Some(name), Some(column)) = (text_at(row, 0), text_at(row, 2)) else {
            continue;
        };
        match indexes.last_mut() {
            Some(index) if index.name == name => index.columns.push(column),
            _ => indexes.push(IndexInfo {
                is_primary: primary_index == Some(name.as_str()),
                is_unique: text_at(row, 1).as_deref() == Some("UNIQUE"),
                columns: vec![column],
                name,
            }),
        }
    }
    indexes
}

fn group_foreign_keys(rows: &[Row], database: Option<&str>) -> Vec<ForeignKeyInfo> {
    let mut keys: Vec<ForeignKeyInfo> = Vec::new();
    for row in rows {
        let (Some(name), Some(column), Some(owner), Some(table), Some(referenced)) = (
            text_at(row, 0),
            text_at(row, 1),
            text_at(row, 2),
            text_at(row, 3),
            text_at(row, 4),
        ) else {
            continue;
        };
        match keys.last_mut() {
            Some(key) if key.name == name => {
                key.columns.push(column);
                key.referenced_columns.push(referenced);
            }
            _ => keys.push(ForeignKeyInfo {
                name,
                columns: vec![column],
                referenced_table: TableRef::new(database.map(str::to_owned), Some(owner), table),
                referenced_columns: vec![referenced],
            }),
        }
    }
    keys
}
