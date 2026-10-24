/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::catalog::{MEASUREMENT_SCHEMA, columns, databases, objects, schemas};
use crate::connection::InfluxDbConnection;
use async_trait::async_trait;
use relix_core::{ColumnInfo, DatabaseInfo, ObjectInfo, ObjectKind, TableDetails, TableRef};
use relix_driver_api::{DriverError, SqlIntrospector};

#[async_trait]
impl SqlIntrospector for InfluxDbConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        databases(&self.endpoint().await?, self.database.as_deref()).await
    }

    async fn schemas(&self, database: &str) -> Result<Vec<String>, DriverError> {
        schemas(&self.endpoint().await?, self.database_or(Some(database))?).await
    }

    async fn objects(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        let database = self.database_or(Some(database))?;
        let schema = schema.unwrap_or(MEASUREMENT_SCHEMA);
        objects(&self.endpoint().await?, database, schema).await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        let endpoint = self.endpoint().await?;
        let database = self.database_or(table.database.as_deref())?;
        let schema = table.schema.as_deref().unwrap_or(MEASUREMENT_SCHEMA);
        let catalog = columns(&endpoint, database, schema, &table.name).await?;
        if catalog.is_empty() {
            return Err(DriverError::query(format!(
                "table {schema}.{} does not exist in {database}",
                table.name
            )));
        }
        let kind = objects(&endpoint, database, schema)
            .await?
            .into_iter()
            .find(|object| object.object.name == table.name)
            .map_or(ObjectKind::Table, |object| object.kind);
        Ok(TableDetails {
            table: TableRef::new(
                Some(database.to_owned()),
                Some(schema.to_owned()),
                table.name.clone(),
            ),
            kind,
            columns: catalog
                .iter()
                .enumerate()
                .map(|(index, column)| ColumnInfo {
                    name: column.name.clone(),
                    data_type: column.data_type.clone(),
                    nullable: column.nullable,
                    default_value: None,
                    ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
                    is_primary_key: false,
                    is_auto_increment: false,
                    comment: Some(column.role(schema).to_owned()),
                })
                .collect(),
            primary_key: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            definition: None,
        })
    }

    async fn object_definition(&self, _object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        Ok(None)
    }
}
