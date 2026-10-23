/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::catalog::{databases, objects};
use crate::connection::Neo4jConnection;
use crate::graph_object::GraphObject;
use crate::table_details::{definition, table_details};
use async_trait::async_trait;
use relix_core::{DatabaseInfo, ObjectInfo, ObjectKind, TableDetails, TableRef};
use relix_driver_api::{DriverError, SqlIntrospector};

#[async_trait]
impl SqlIntrospector for Neo4jConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        let live = self.live().await?;
        databases(&live.graph, &self.default_database).await
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
        objects(&live.graph, self.database_name(Some(database))).await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        let live = self.live().await?;
        let database = self.database_name(table.database.as_deref());
        let object = GraphObject::resolve(&live.graph, database, table).await?;
        table_details(&live.graph, database, &object).await
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        if !matches!(
            object.kind,
            ObjectKind::NodeLabel | ObjectKind::RelationshipType
        ) {
            return Ok(None);
        }
        let live = self.live().await?;
        let database = self.database_name(object.object.database.as_deref());
        let resolved = GraphObject::resolve(&live.graph, database, &object.object).await?;
        definition(&live.graph, database, &resolved).await
    }
}
