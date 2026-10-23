/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::{keyspaces, objects};
use crate::connection::CassandraConnection;
use crate::definitions::describe;
use crate::table_details::table_details;
use crate::table_names::keyspace_of;
use async_trait::async_trait;
use relix_core::{DatabaseInfo, ObjectInfo, TableDetails, TableRef};
use relix_driver_api::{DriverError, SqlIntrospector};

#[async_trait]
impl SqlIntrospector for CassandraConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        let live = self.live().await?;
        keyspaces(&live.session, self.default_keyspace.as_deref()).await
    }

    async fn schemas(&self, _database: &str) -> Result<Vec<String>, DriverError> {
        Ok(Vec::new())
    }

    async fn objects(
        &self,
        database: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        let live = self.live().await?;
        objects(&live.session, database).await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        let live = self.live().await?;
        let keyspace = keyspace_of(table, self.default_keyspace.as_deref())?;
        table_details(&live.session, keyspace, table).await
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        let live = self.live().await?;
        let keyspace = keyspace_of(&object.object, self.default_keyspace.as_deref())?;
        Ok(describe(&live.session, keyspace, &object.object.name, object.kind).await)
    }
}
