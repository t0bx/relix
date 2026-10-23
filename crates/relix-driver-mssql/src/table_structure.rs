/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::object_kind;
use crate::catalog_rows::{flag_at, in_database, int_at, query_rows, text_at};
use crate::column_type::declared_type;
use crate::connector::MsSqlClient;
use relix_core::{ColumnInfo, ForeignKeyInfo, IndexInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;
use tiberius::Row;

const OBJECT_TYPE_SQL: &str = "SELECT RTRIM(o.type) FROM {db}.sys.objects o \
     WHERE o.object_id = OBJECT_ID(@P1)";

const COLUMNS_SQL: &str = "SELECT c.name, t.name, CAST(c.max_length AS int), \
       CAST(c.precision AS int), CAST(c.scale AS int), c.is_nullable, dc.definition, \
       c.is_identity, CAST(c.column_id AS int), CAST(ep.value AS nvarchar(4000)), \
       cc.definition, CAST(ic.seed_value AS nvarchar(64)), \
       CAST(ic.increment_value AS nvarchar(64)), t.is_user_defined \
     FROM {db}.sys.columns c \
     JOIN {db}.sys.types t ON t.user_type_id = c.user_type_id \
     LEFT JOIN {db}.sys.default_constraints dc ON dc.object_id = c.default_object_id \
     LEFT JOIN {db}.sys.computed_columns cc ON cc.object_id = c.object_id \
       AND cc.column_id = c.column_id \
     LEFT JOIN {db}.sys.identity_columns ic ON ic.object_id = c.object_id \
       AND ic.column_id = c.column_id \
     LEFT JOIN {db}.sys.extended_properties ep ON ep.class = 1 \
       AND ep.major_id = c.object_id AND ep.minor_id = c.column_id \
       AND ep.name = N'MS_Description' \
     WHERE c.object_id = OBJECT_ID(@P1) \
     ORDER BY c.column_id";

const INDEXES_SQL: &str = "SELECT i.name, i.is_unique, i.is_primary_key, \
       i.is_unique_constraint, CAST(CASE WHEN i.type = 1 THEN 1 ELSE 0 END AS bit), \
       col.name, ic.is_descending_key \
     FROM {db}.sys.indexes i \
     JOIN {db}.sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
     JOIN {db}.sys.columns col ON col.object_id = ic.object_id AND col.column_id = ic.column_id \
     WHERE i.object_id = OBJECT_ID(@P1) AND i.type IN (1, 2) \
       AND ic.is_included_column = 0 AND i.is_hypothetical = 0 \
     ORDER BY i.is_primary_key DESC, i.index_id, ic.key_ordinal";

const FOREIGN_KEYS_SQL: &str = "SELECT fk.name, pc.name, rs.name, rt.name, rc.name, \
       fk.delete_referential_action_desc, fk.update_referential_action_desc \
     FROM {db}.sys.foreign_keys fk \
     JOIN {db}.sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id \
     JOIN {db}.sys.columns pc ON pc.object_id = fkc.parent_object_id \
       AND pc.column_id = fkc.parent_column_id \
     JOIN {db}.sys.objects rt ON rt.object_id = fkc.referenced_object_id \
     JOIN {db}.sys.schemas rs ON rs.schema_id = rt.schema_id \
     JOIN {db}.sys.columns rc ON rc.object_id = fkc.referenced_object_id \
       AND rc.column_id = fkc.referenced_column_id \
     WHERE fk.parent_object_id = OBJECT_ID(@P1) \
     ORDER BY fk.name, fkc.constraint_column_id";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDefinition {
    pub info: ColumnInfo,
    pub computed: Option<String>,
    pub identity: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDefinition {
    pub name: String,
    pub is_unique: bool,
    pub is_primary: bool,
    pub is_unique_constraint: bool,
    pub is_clustered: bool,
    pub columns: Vec<(String, bool)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKeyDefinition {
    pub info: ForeignKeyInfo,
    pub on_delete: String,
    pub on_update: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableStructure {
    pub columns: Vec<ColumnDefinition>,
    pub indexes: Vec<IndexDefinition>,
    pub foreign_keys: Vec<ForeignKeyDefinition>,
}

impl IndexDefinition {
    pub fn info(&self) -> IndexInfo {
        IndexInfo {
            name: self.name.clone(),
            columns: self.columns.iter().map(|(name, _)| name.clone()).collect(),
            is_unique: self.is_unique,
            is_primary: self.is_primary,
        }
    }
}

pub fn primary_key(indexes: &[IndexDefinition]) -> Vec<String> {
    indexes
        .iter()
        .find(|index| index.is_primary)
        .map(|index| index.info().columns)
        .unwrap_or_default()
}

pub async fn object_kind_of(
    client: &mut MsSqlClient,
    table: &TableRef,
    object_name: &str,
) -> Result<ObjectKind, DriverError> {
    let database = table.database.as_deref().unwrap_or_default();
    let rows = query_rows(
        client,
        &in_database(OBJECT_TYPE_SQL, database),
        &[&object_name],
    )
    .await?;
    rows.first()
        .and_then(|row| text_at(row, 0))
        .and_then(|object_type| object_kind(&object_type))
        .ok_or_else(|| DriverError::query(format!("object {table} does not exist")))
}

pub async fn table_structure(
    client: &mut MsSqlClient,
    table: &TableRef,
    object_name: &str,
) -> Result<TableStructure, DriverError> {
    let database = table.database.as_deref().unwrap_or_default();
    let params: [&dyn tiberius::ToSql; 1] = [&object_name];
    let index_rows = query_rows(client, &in_database(INDEXES_SQL, database), &params).await?;
    let indexes = group_indexes(&index_rows);
    let primary_key = primary_key(&indexes);
    let column_rows = query_rows(client, &in_database(COLUMNS_SQL, database), &params).await?;
    let columns = column_rows
        .iter()
        .filter_map(|row| column_definition(row, &primary_key))
        .collect();
    let key_rows = query_rows(client, &in_database(FOREIGN_KEYS_SQL, database), &params).await?;
    Ok(TableStructure {
        columns,
        indexes,
        foreign_keys: group_foreign_keys(&key_rows, database),
    })
}

fn column_definition(row: &Row, primary_key: &[String]) -> Option<ColumnDefinition> {
    let name = text_at(row, 0)?;
    let is_identity = flag_at(row, 7);
    let info = ColumnInfo {
        data_type: declared_type(
            &text_at(row, 1)?,
            int_at(row, 2).unwrap_or_default(),
            int_at(row, 3).unwrap_or_default(),
            int_at(row, 4).unwrap_or_default(),
            flag_at(row, 13),
        ),
        nullable: flag_at(row, 5),
        default_value: text_at(row, 6),
        ordinal: u32::try_from(int_at(row, 8).unwrap_or_default()).unwrap_or_default(),
        is_primary_key: primary_key.contains(&name),
        is_auto_increment: is_identity,
        comment: text_at(row, 9).filter(|comment| !comment.is_empty()),
        name,
    };
    let identity = is_identity.then(|| {
        (
            text_at(row, 11).unwrap_or_else(|| "1".to_owned()),
            text_at(row, 12).unwrap_or_else(|| "1".to_owned()),
        )
    });
    Some(ColumnDefinition {
        info,
        computed: text_at(row, 10),
        identity,
    })
}

fn group_indexes(rows: &[Row]) -> Vec<IndexDefinition> {
    let mut indexes: Vec<IndexDefinition> = Vec::new();
    for row in rows {
        let (Some(name), Some(column)) = (text_at(row, 0), text_at(row, 5)) else {
            continue;
        };
        let entry = (column, flag_at(row, 6));
        match indexes.last_mut() {
            Some(index) if index.name == name => index.columns.push(entry),
            _ => indexes.push(IndexDefinition {
                name,
                is_unique: flag_at(row, 1),
                is_primary: flag_at(row, 2),
                is_unique_constraint: flag_at(row, 3),
                is_clustered: flag_at(row, 4),
                columns: vec![entry],
            }),
        }
    }
    indexes
}

fn group_foreign_keys(rows: &[Row], database: &str) -> Vec<ForeignKeyDefinition> {
    let mut keys: Vec<ForeignKeyDefinition> = Vec::new();
    for row in rows {
        let (Some(name), Some(column), Some(schema), Some(table), Some(referenced)) = (
            text_at(row, 0),
            text_at(row, 1),
            text_at(row, 2),
            text_at(row, 3),
            text_at(row, 4),
        ) else {
            continue;
        };
        match keys.last_mut() {
            Some(key) if key.info.name == name => {
                key.info.columns.push(column);
                key.info.referenced_columns.push(referenced);
            }
            _ => keys.push(ForeignKeyDefinition {
                info: ForeignKeyInfo {
                    name,
                    columns: vec![column],
                    referenced_table: TableRef::new(Some(database.to_owned()), Some(schema), table),
                    referenced_columns: vec![referenced],
                },
                on_delete: text_at(row, 5).unwrap_or_default(),
                on_update: text_at(row, 6).unwrap_or_default(),
            }),
        }
    }
    keys
}
