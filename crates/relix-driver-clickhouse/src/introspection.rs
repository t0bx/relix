/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::{list_databases, list_objects, table_engine};
use crate::connection::ClickHouseConnection;
use crate::definitions::show_create;
use crate::table_details::load_table_details;
use async_trait::async_trait;
use relix_core::{DatabaseInfo, ObjectInfo, TableDetails, TableRef};
use relix_driver_api::{DriverError, SqlIntrospector};

impl ClickHouseConnection {
    fn database_of<'a>(&'a self, table: &'a TableRef) -> &'a str {
        table
            .database
            .as_deref()
            .or(table.schema.as_deref())
            .filter(|database| !database.is_empty())
            .unwrap_or(&self.default_database)
    }
}

#[async_trait]
impl SqlIntrospector for ClickHouseConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        list_databases(&self.client, &self.default_database).await
    }

    async fn schemas(&self, _database: &str) -> Result<Vec<String>, DriverError> {
        Ok(Vec::new())
    }

    async fn objects(
        &self,
        database: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        list_objects(&self.client, database).await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        load_table_details(&self.client, table, self.database_of(table)).await
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        let database = self.database_of(&object.object);
        let Some(engine) = table_engine(&self.client, database, &object.object.name).await? else {
            return Ok(None);
        };
        let table = TableRef::new(Some(database.to_owned()), None, object.object.name.clone());
        show_create(&self.client, &table, &engine).await
    }
}
