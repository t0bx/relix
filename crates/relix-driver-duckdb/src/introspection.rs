/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::{list_databases, list_objects, list_schemas};
use crate::connection::DuckDbConnection;
use crate::definitions::{DEFAULT_SCHEMA, object_definition};
use crate::table_details::load_table_details;
use async_trait::async_trait;
use relix_core::{DatabaseInfo, ObjectInfo, TableDetails, TableRef};
use relix_driver_api::{DriverError, SqlIntrospector};

#[async_trait]
impl SqlIntrospector for DuckDbConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        self.data.run(|connection| list_databases(connection)).await
    }

    async fn schemas(&self, database: &str) -> Result<Vec<String>, DriverError> {
        let database = database.to_owned();
        self.data
            .run(move |connection| list_schemas(connection, &database))
            .await
    }

    async fn objects(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        let database = database.to_owned();
        let schema = schema.unwrap_or(DEFAULT_SCHEMA).to_owned();
        self.data
            .run(move |connection| list_objects(connection, &database, &schema))
            .await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        let table = table.clone();
        self.data
            .run(move |connection| load_table_details(connection, &table))
            .await
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        let object = object.clone();
        self.data
            .run(move |connection| object_definition(connection, &object))
            .await
    }
}
