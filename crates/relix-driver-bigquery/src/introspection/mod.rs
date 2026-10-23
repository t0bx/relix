/*
 * @author t0bx
 * @since 23.10.2026
 */

mod definitions;
pub(crate) mod objects;
mod table_details;

use crate::api::catalog;
use crate::connection::BigQueryConnection;
use async_trait::async_trait;
use relix_core::{DatabaseInfo, ObjectInfo, TableDetails, TableRef};
use relix_driver_api::{DriverError, SqlIntrospector};

#[async_trait]
impl SqlIntrospector for BigQueryConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        let mut databases = vec![DatabaseInfo {
            name: self.project.clone(),
            is_default: true,
        }];
        for project in &self.additional_projects {
            if !databases.iter().any(|database| &database.name == project) {
                databases.push(DatabaseInfo {
                    name: project.clone(),
                    is_default: false,
                });
            }
        }
        Ok(databases)
    }

    async fn schemas(&self, database: &str) -> Result<Vec<String>, DriverError> {
        let datasets = catalog::datasets(self.client(), database).await?;
        Ok(datasets
            .into_iter()
            .map(|entry| entry.dataset_reference.dataset_id)
            .collect())
    }

    async fn objects(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        let dataset = schema
            .ok_or_else(|| DriverError::internal("BigQuery objects are listed per dataset"))?;
        objects::list(self, database, dataset).await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        table_details::describe(self, table).await
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        definitions::ddl(self, object).await
    }
}

impl BigQueryConnection {
    /// Project and dataset of a table reference; the project defaults to the connection's.
    pub(crate) fn table_scope(&self, table: &TableRef) -> Result<(String, String), DriverError> {
        let dataset = table.schema.clone().ok_or_else(|| {
            DriverError::internal(format!("{table} is not qualified with a dataset"))
        })?;
        let project = table
            .database
            .clone()
            .unwrap_or_else(|| self.project.clone());
        Ok((project, dataset))
    }
}
