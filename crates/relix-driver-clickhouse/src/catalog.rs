/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::{ClickHouseClient, QueryRequest};
use crate::metadata_rows::{optional_text_at, text_at};
use relix_core::{DatabaseInfo, ObjectInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;

const DATABASES_SQL: &str = "SELECT name FROM system.databases ORDER BY name";
const OBJECTS_SQL: &str = "SELECT name, engine, comment FROM system.tables \
     WHERE database = {database:String} AND NOT is_temporary ORDER BY name";
const ENGINE_SQL: &str = "SELECT engine FROM system.tables \
     WHERE database = {database:String} AND name = {table:String}";

pub(crate) async fn list_databases(
    client: &ClickHouseClient,
    default_database: &str,
) -> Result<Vec<DatabaseInfo>, DriverError> {
    let batch = client.rows(&QueryRequest::new(DATABASES_SQL)).await?;
    Ok(batch
        .rows
        .iter()
        .map(|row| {
            let name = text_at(row, 0);
            DatabaseInfo {
                is_default: name == default_database,
                name,
            }
        })
        .collect())
}

pub(crate) async fn list_objects(
    client: &ClickHouseClient,
    database: &str,
) -> Result<Vec<ObjectInfo>, DriverError> {
    let request = QueryRequest::new(OBJECTS_SQL).param("database", database);
    let batch = client.rows(&request).await?;
    Ok(batch
        .rows
        .iter()
        .map(|row| ObjectInfo {
            object: TableRef::new(Some(database.to_owned()), None, text_at(row, 0)),
            kind: kind_of_engine(&text_at(row, 1)),
            comment: optional_text_at(row, 2),
        })
        .collect())
}

pub(crate) async fn table_engine(
    client: &ClickHouseClient,
    database: &str,
    table: &str,
) -> Result<Option<String>, DriverError> {
    let request = QueryRequest::new(ENGINE_SQL)
        .param("database", database)
        .param("table", table);
    let batch = client.rows(&request).await?;
    Ok(batch.rows.first().map(|row| text_at(row, 0)))
}

pub(crate) fn kind_of_engine(engine: &str) -> ObjectKind {
    match engine {
        "View" | "LiveView" | "WindowView" => ObjectKind::View,
        "MaterializedView" => ObjectKind::MaterializedView,
        _ => ObjectKind::Table,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engines_map_to_object_kinds() {
        assert_eq!(kind_of_engine("MergeTree"), ObjectKind::Table);
        assert_eq!(kind_of_engine("Dictionary"), ObjectKind::Table);
        assert_eq!(kind_of_engine("View"), ObjectKind::View);
        assert_eq!(
            kind_of_engine("MaterializedView"),
            ObjectKind::MaterializedView
        );
    }
}
