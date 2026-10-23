/*
 * @author t0bx
 * @since 23.10.2026
 */

mod catalog;
mod ddl;
mod definitions;
mod table_details;

use crate::connection::PostgresConnection;
use async_trait::async_trait;
use relix_core::{DatabaseInfo, ObjectInfo, ObjectKind, TableDetails, TableRef};
use relix_driver_api::{DriverError, SqlIntrospector};

#[async_trait]
impl SqlIntrospector for PostgresConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        let client = self.data_client(None).await?;
        let guard = client.read().await;
        catalog::list_databases(&guard, &self.default_database).await
    }

    async fn schemas(&self, database: &str) -> Result<Vec<String>, DriverError> {
        let client = self.data_client(Some(database)).await?;
        let guard = client.read().await;
        catalog::list_schemas(&guard).await
    }

    async fn objects(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        let database = self.database_name(Some(database));
        let client = self.data_client(Some(database)).await?;
        let guard = client.read().await;
        catalog::list_objects(&guard, database, schema).await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        let database = self.database_name(table.database.as_deref());
        let client = self.data_client(Some(database)).await?;
        let guard = client.read().await;
        table_details::load_table_details(&guard, &self.builder, database, table).await
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        let target = &object.object;
        let database = self.database_name(target.database.as_deref());
        let client = self.data_client(Some(database)).await?;
        let guard = client.read().await;
        match object.kind {
            ObjectKind::Table => {
                table_details::load_table_details(&guard, &self.builder, database, target)
                    .await
                    .map(|details| details.definition)
            }
            ObjectKind::View | ObjectKind::MaterializedView => {
                match table_details::find_relation(&guard, database, target).await? {
                    Some(relation) => {
                        let qualified_name = self.builder.qualified_table(&relation.table);
                        table_details::view_ddl(&guard, &relation, &qualified_name)
                            .await
                            .map(Some)
                    }
                    None => Ok(None),
                }
            }
            ObjectKind::Function | ObjectKind::Procedure => {
                definitions::routine_definition(&guard, target, object.kind).await
            }
            ObjectKind::Sequence => {
                definitions::sequence_definition(&guard, &self.builder, target).await
            }
            ObjectKind::Trigger | ObjectKind::NodeLabel | ObjectKind::RelationshipType => Ok(None),
        }
    }
}
