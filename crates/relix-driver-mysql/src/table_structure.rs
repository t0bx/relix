/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::relation_kind;
use crate::catalog_rows::{catalog_query, non_empty_text_at, number_at, text_at};
use crate::flavor::Flavor;
use mysql_async::{Conn, Row};
use relix_core::{ColumnInfo, ForeignKeyInfo, IndexInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;

const PRIMARY_INDEX: &str = "PRIMARY";
const EXPRESSION_COLUMN: &str = "<expression>";

const KIND_SQL: &str = "SELECT TABLE_TYPE FROM information_schema.TABLES \
     WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?";

const COLUMNS_SQL: &str = "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, \
     ORDINAL_POSITION, EXTRA, COLUMN_COMMENT \
     FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
     ORDER BY ORDINAL_POSITION";

const INDEXES_SQL: &str = "SELECT INDEX_NAME, COLUMN_NAME, NON_UNIQUE \
     FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
     ORDER BY INDEX_NAME = 'PRIMARY' DESC, INDEX_NAME, SEQ_IN_INDEX";

const FOREIGN_KEYS_SQL: &str = "SELECT k.CONSTRAINT_NAME, k.COLUMN_NAME, \
     k.REFERENCED_TABLE_SCHEMA, k.REFERENCED_TABLE_NAME, k.REFERENCED_COLUMN_NAME \
     FROM information_schema.KEY_COLUMN_USAGE k \
     JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
       ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA \
      AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME \
      AND r.TABLE_NAME = k.TABLE_NAME \
     WHERE k.TABLE_SCHEMA = ? AND k.TABLE_NAME = ? AND k.REFERENCED_TABLE_NAME IS NOT NULL \
     ORDER BY k.CONSTRAINT_NAME, k.ORDINAL_POSITION";

pub async fn relation_kind_of(
    conn: &mut Conn,
    table: &TableRef,
) -> Result<ObjectKind, DriverError> {
    let rows = catalog_query(conn, KIND_SQL, &table_params(table)).await?;
    rows.first()
        .and_then(|row| text_at(row, 0))
        .and_then(|table_type| relation_kind(&table_type))
        .ok_or_else(|| DriverError::query(format!("table {table} does not exist")))
}

pub async fn columns(
    conn: &mut Conn,
    table: &TableRef,
    flavor: Flavor,
    primary_key: &[String],
) -> Result<Vec<ColumnInfo>, DriverError> {
    let rows = catalog_query(conn, COLUMNS_SQL, &table_params(table)).await?;
    Ok(rows
        .iter()
        .filter_map(|row| column_info(row, flavor, primary_key))
        .collect())
}

pub async fn indexes(conn: &mut Conn, table: &TableRef) -> Result<Vec<IndexInfo>, DriverError> {
    let rows = catalog_query(conn, INDEXES_SQL, &table_params(table)).await?;
    let mut indexes: Vec<IndexInfo> = Vec::new();
    for row in &rows {
        let Some(name) = text_at(row, 0) else {
            continue;
        };
        let column = text_at(row, 1).unwrap_or_else(|| EXPRESSION_COLUMN.to_owned());
        match indexes.last_mut() {
            Some(index) if index.name == name => index.columns.push(column),
            _ => indexes.push(IndexInfo {
                is_primary: name == PRIMARY_INDEX,
                is_unique: number_at(row, 2) == Some(0),
                columns: vec![column],
                name,
            }),
        }
    }
    Ok(indexes)
}

pub async fn foreign_keys(
    conn: &mut Conn,
    table: &TableRef,
) -> Result<Vec<ForeignKeyInfo>, DriverError> {
    let rows = catalog_query(conn, FOREIGN_KEYS_SQL, &table_params(table)).await?;
    let mut keys: Vec<ForeignKeyInfo> = Vec::new();
    for row in &rows {
        let (Some(name), Some(column), Some(referenced_name), Some(referenced_column)) = (
            text_at(row, 0),
            text_at(row, 1),
            text_at(row, 3),
            text_at(row, 4),
        ) else {
            continue;
        };
        match keys.last_mut() {
            Some(key) if key.name == name => {
                key.columns.push(column);
                key.referenced_columns.push(referenced_column);
            }
            _ => keys.push(ForeignKeyInfo {
                name,
                columns: vec![column],
                referenced_table: TableRef::new(text_at(row, 2), None, referenced_name),
                referenced_columns: vec![referenced_column],
            }),
        }
    }
    Ok(keys)
}

pub fn primary_key(indexes: &[IndexInfo]) -> Vec<String> {
    indexes
        .iter()
        .find(|index| index.is_primary)
        .map(|index| index.columns.clone())
        .unwrap_or_default()
}

fn table_params(table: &TableRef) -> [&str; 2] {
    [table.database.as_deref().unwrap_or_default(), &table.name]
}

fn column_info(row: &Row, flavor: Flavor, primary_key: &[String]) -> Option<ColumnInfo> {
    let name = text_at(row, 0)?;
    let extra = text_at(row, 5).unwrap_or_default();
    Some(ColumnInfo {
        is_primary_key: primary_key.contains(&name),
        data_type: text_at(row, 1).unwrap_or_default(),
        nullable: text_at(row, 2).is_some_and(|nullable| nullable == "YES"),
        default_value: normalize_default(flavor, text_at(row, 3)),
        ordinal: number_at(row, 4)
            .and_then(|ordinal| u32::try_from(ordinal).ok())
            .unwrap_or_default(),
        is_auto_increment: extra.to_ascii_lowercase().contains("auto_increment"),
        comment: non_empty_text_at(row, 6),
        name,
    })
}

/// MariaDB reports a missing default as the literal `NULL` (string defaults are quoted),
/// whereas MySQL reports SQL NULL.
fn normalize_default(flavor: Flavor, default: Option<String>) -> Option<String> {
    match flavor {
        Flavor::MariaDb => default.filter(|value| value != "NULL"),
        Flavor::MySql | Flavor::TiDb => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mariadb_null_defaults_are_dropped() {
        assert_eq!(
            normalize_default(Flavor::MariaDb, Some("NULL".into())),
            None
        );
        assert_eq!(
            normalize_default(Flavor::MariaDb, Some("'NULL'".into())),
            Some("'NULL'".into())
        );
        assert_eq!(
            normalize_default(Flavor::MySql, Some("NULL".into())),
            Some("NULL".into())
        );
    }

    #[test]
    fn primary_key_comes_from_primary_index() {
        let indexes = vec![IndexInfo {
            name: PRIMARY_INDEX.into(),
            columns: vec!["a".into(), "b".into()],
            is_unique: true,
            is_primary: true,
        }];
        assert_eq!(primary_key(&indexes), vec!["a".to_owned(), "b".to_owned()]);
        assert!(primary_key(&[]).is_empty());
    }
}
