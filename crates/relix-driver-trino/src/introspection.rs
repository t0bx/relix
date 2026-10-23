/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::TrinoConnection;
use async_trait::async_trait;
use relix_core::{DatabaseInfo, ObjectInfo, TableDetails, TableRef};
use relix_driver_api::{DriverError, SqlIntrospector};

#[async_trait]
impl SqlIntrospector for TrinoConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        self.list_catalogs().await
    }

    async fn schemas(&self, database: &str) -> Result<Vec<String>, DriverError> {
        self.list_schemas(database).await
    }

    async fn objects(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        let schema = schema
            .map(str::to_owned)
            .or_else(|| self.initial_session.schema.clone())
            .ok_or_else(|| DriverError::query("select a schema to list its tables"))?;
        self.list_objects(database, &schema).await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        self.load_table_details(table).await
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        let qualified = self.qualify(&object.object)?;
        match self.relation_kind(&qualified).await? {
            Some(kind) => self.definition(&qualified, kind).await,
            None => Ok(None),
        }
    }
}
