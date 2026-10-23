/*
 * @author t0bx
 * @since 23.10.2026
 */

mod definitions;
mod objects;
mod show_table;
mod table_details;

use crate::connection::SnowflakeConnection;
use async_trait::async_trait;
use relix_core::{DatabaseInfo, Dialect, ObjectInfo, TableDetails, TableRef};
use relix_driver_api::{DriverError, SqlIntrospector};
use show_table::ShowTable;

#[async_trait]
impl SqlIntrospector for SnowflakeConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        let databases = show(self, "SHOW DATABASES").await?;
        let configured = self.context().database;
        Ok(databases
            .rows()
            .filter_map(|row| {
                let name = row.get("name")?;
                let is_default = match configured.as_deref() {
                    Some(configured) => configured == name,
                    None => row.flag("is_default"),
                };
                Some(DatabaseInfo {
                    name: name.to_owned(),
                    is_default,
                })
            })
            .collect())
    }

    async fn schemas(&self, database: &str) -> Result<Vec<String>, DriverError> {
        let sql = format!("SHOW TERSE SCHEMAS IN DATABASE {}", identifier(database));
        let schemas = show(self, &sql).await?;
        Ok(schemas
            .rows()
            .filter_map(|row| row.get("name").map(str::to_owned))
            .collect())
    }

    async fn objects(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        let schema = schema
            .map(str::to_owned)
            .or_else(|| self.context().schema)
            .unwrap_or_else(|| "PUBLIC".to_owned());
        objects::list(self, database, &schema).await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        table_details::describe(self, table).await
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        definitions::ddl(self, object).await
    }
}

pub(crate) async fn show(
    connection: &SnowflakeConnection,
    sql: &str,
) -> Result<ShowTable, DriverError> {
    connection
        .query_rows(sql, usize::MAX)
        .await
        .map(ShowTable::from)
}

pub(crate) fn identifier(name: &str) -> String {
    Dialect::Snowflake.quote_identifier(name)
}

/// Snowflake string literals treat backslashes as escapes, so they are doubled along
/// with single quotes.
pub(crate) fn string_literal(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "''"))
}

pub(crate) fn scope(table: &TableRef) -> Result<(String, String), DriverError> {
    match (&table.database, &table.schema) {
        (Some(database), Some(schema)) => Ok((database.clone(), schema.clone())),
        _ => Err(DriverError::internal(format!(
            "{table} is not qualified with database and schema"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_escape_quotes_and_backslashes() {
        assert_eq!(string_literal("it's a\\b"), "'it''s a\\\\b'");
        assert_eq!(identifier("my\"db"), "\"my\"\"db\"");
    }
}
