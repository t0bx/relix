/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::SnowflakeConnection;
use crate::introspection::objects::relation_kind;
use crate::introspection::show_table::ShowTable;
use crate::introspection::{identifier, scope, show, string_literal};
use relix_core::{ColumnInfo, ForeignKeyInfo, ObjectKind, TableDetails, TableRef};
use relix_driver_api::DriverError;

/// Describes a table or view: its kind from `SHOW TERSE OBJECTS`, columns and primary
/// key from `DESC TABLE|VIEW` and foreign keys from `SHOW IMPORTED KEYS`. Snowflake has
/// no secondary indexes and does not enforce key constraints.
pub(crate) async fn describe(
    connection: &SnowflakeConnection,
    table: &TableRef,
) -> Result<TableDetails, DriverError> {
    let (database, schema) = scope(table)?;
    let qualified = connection.builder.qualified_table(table);
    let lookup = format!(
        "SHOW TERSE OBJECTS LIKE {} IN SCHEMA {}.{}",
        string_literal(&table.name),
        identifier(&database),
        identifier(&schema)
    );
    let kind = show(connection, &lookup)
        .await?
        .rows()
        .find(|row| row.get("name") == Some(table.name.as_str()))
        .map_or(ObjectKind::Table, |row| {
            relation_kind(row.get("kind").unwrap_or_default())
        });
    let describe_keyword = if kind == ObjectKind::Table {
        "TABLE"
    } else {
        "VIEW"
    };
    let describe_sql = format!("DESC {describe_keyword} {qualified}");
    let keys_sql = format!("SHOW IMPORTED KEYS IN TABLE {qualified}");
    let (described, imported) = tokio::try_join!(show(connection, &describe_sql), async {
        match kind {
            ObjectKind::Table => show(connection, &keys_sql).await.map(Some),
            _ => Ok(None),
        }
    })?;
    let columns = columns(&described);
    Ok(TableDetails {
        table: table.clone(),
        kind,
        primary_key: columns
            .iter()
            .filter(|column| column.is_primary_key)
            .map(|column| column.name.clone())
            .collect(),
        columns,
        indexes: Vec::new(),
        foreign_keys: imported.as_ref().map(foreign_keys).unwrap_or_default(),
        definition: None,
    })
}

fn columns(described: &ShowTable) -> Vec<ColumnInfo> {
    described
        .rows()
        .filter_map(|row| {
            let default_value = row.non_empty("default");
            let generated = default_value.as_deref().is_some_and(|default| {
                let upper = default.to_ascii_uppercase();
                upper.contains("IDENTITY") || upper.contains("AUTOINCREMENT")
            });
            Some(ColumnInfo {
                name: row.get("name")?.to_owned(),
                data_type: row.get("type").unwrap_or_default().to_owned(),
                nullable: row.flag("null?"),
                default_value,
                ordinal: 0,
                is_primary_key: row.flag("primary key"),
                is_auto_increment: generated,
                comment: row.non_empty("comment"),
            })
        })
        .enumerate()
        .map(|(index, column)| ColumnInfo {
            ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
            ..column
        })
        .collect()
}

fn foreign_keys(imported: &ShowTable) -> Vec<ForeignKeyInfo> {
    let mut keys: Vec<(ForeignKeyInfo, Vec<u32>)> = Vec::new();
    for row in imported.rows() {
        let (Some(name), Some(column), Some(referenced)) = (
            row.get("fk_name"),
            row.get("fk_column_name"),
            row.get("pk_column_name"),
        ) else {
            continue;
        };
        let sequence = row
            .get("key_sequence")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let position = keys.iter().position(|(key, _)| key.name == name);
        let index = position.unwrap_or_else(|| {
            keys.push((
                ForeignKeyInfo {
                    name: name.to_owned(),
                    columns: Vec::new(),
                    referenced_table: TableRef::new(
                        row.non_empty("pk_database_name"),
                        row.non_empty("pk_schema_name"),
                        row.get("pk_table_name").unwrap_or_default(),
                    ),
                    referenced_columns: Vec::new(),
                },
                Vec::new(),
            ));
            keys.len() - 1
        });
        let (key, sequences) = &mut keys[index];
        let slot = sequences.partition_point(|existing| *existing <= sequence);
        sequences.insert(slot, sequence);
        key.columns.insert(slot, column.to_owned());
        key.referenced_columns.insert(slot, referenced.to_owned());
    }
    keys.into_iter().map(|(key, _)| key).collect()
}
