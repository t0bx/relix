/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::{database_of, list_databases, list_objects, master_entry};
use crate::connection::SqliteConnection;
use crate::table_details::load_table_details;
use async_trait::async_trait;
use relix_core::{DatabaseInfo, ObjectInfo, TableDetails, TableRef};
use relix_driver_api::{DriverError, SqlIntrospector};

#[async_trait]
impl SqlIntrospector for SqliteConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        self.shared
            .run(|connection| list_databases(connection))
            .await
    }

    async fn schemas(&self, _database: &str) -> Result<Vec<String>, DriverError> {
        Ok(Vec::new())
    }

    async fn objects(
        &self,
        database: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        let database = database.to_owned();
        self.shared
            .run(move |connection| list_objects(connection, &database))
            .await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        let table = table.clone();
        self.shared
            .run(move |connection| load_table_details(connection, &table))
            .await
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        let object = object.clone();
        self.shared
            .run(move |connection| {
                let database = database_of(&object.object);
                let entry =
                    master_entry(connection, database, &object.object.name, &[object.kind])?;
                Ok(entry.and_then(|entry| entry.sql))
            })
            .await
    }
}
