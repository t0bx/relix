/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::{list_databases, list_objects, list_schemas};
use crate::connection::{MsSqlConnection, sql_builder};
use crate::connector::MsSqlClient;
use crate::ddl::create_table;
use crate::definitions::{module_definition, object_definition};
use crate::table_structure::{object_kind_of, primary_key, table_structure};
use async_trait::async_trait;
use relix_core::{DatabaseInfo, ObjectInfo, ObjectKind, TableDetails, TableRef};
use relix_driver_api::{DriverError, SqlIntrospector};

#[async_trait]
impl SqlIntrospector for MsSqlConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        let mut lease = self.data.lease().await?;
        let names = list_databases(lease.client()?).await;
        let names = lease.finish(names)?;
        Ok(names
            .into_iter()
            .map(|name| DatabaseInfo {
                is_default: name == self.default_database,
                name,
            })
            .collect())
    }

    async fn schemas(&self, database: &str) -> Result<Vec<String>, DriverError> {
        let database = self.resolve_database(Some(database));
        let mut lease = self.data.lease().await?;
        let schemas = list_schemas(lease.client()?, &database).await;
        lease.finish(schemas)
    }

    async fn objects(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        let database = self.resolve_database(Some(database));
        let mut lease = self.data.lease().await?;
        let objects = list_objects(lease.client()?, &database, schema).await;
        lease.finish(objects)
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        let table = self.resolve_table(table);
        let mut lease = self.data.lease().await?;
        let details = table_details_of(lease.client()?, &table).await;
        lease.finish(details)
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        let target = self.resolve_table(&object.object);
        let mut lease = self.data.lease().await?;
        let definition = object_definition(lease.client()?, object.kind, &target).await;
        lease.finish(definition)
    }
}

async fn table_details_of(
    client: &mut MsSqlClient,
    table: &TableRef,
) -> Result<TableDetails, DriverError> {
    let object_name = sql_builder().qualified_table(table);
    let kind = object_kind_of(client, table, &object_name).await?;
    let structure = table_structure(client, table, &object_name).await?;
    let definition = match kind {
        ObjectKind::Table => Some(create_table(table, &structure)),
        _ => module_definition(client, table, &object_name).await?,
    };
    Ok(TableDetails {
        table: table.clone(),
        kind,
        primary_key: primary_key(&structure.indexes),
        columns: structure
            .columns
            .into_iter()
            .map(|column| column.info)
            .collect(),
        indexes: structure.indexes.iter().map(|index| index.info()).collect(),
        foreign_keys: structure
            .foreign_keys
            .into_iter()
            .map(|key| key.info)
            .collect(),
        definition,
    })
}
