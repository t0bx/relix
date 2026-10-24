/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::catalog::{MEASUREMENT_SCHEMA, columns};
use crate::connection::InfluxDbConnection;
use crate::page_statement::{count_statement, page_statement};
use crate::query_client::query_records;
use crate::query_language::QueryLanguage;
use crate::result_batch::{ResultColumn, build_batch};
use crate::values::is_timestamp_type;
use async_trait::async_trait;
use relix_core::{ChangeReport, ChangeSet, RowBatch, TableQuery, TableRef};
use relix_driver_api::{DriverError, TableDataAccess};

#[async_trait]
impl TableDataAccess for InfluxDbConnection {
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        let endpoint = self.endpoint().await?;
        let database = self.database_or(table.database.as_deref())?;
        let schema = table.schema.as_deref().unwrap_or(MEASUREMENT_SCHEMA);
        let catalog = columns(&endpoint, database, schema, &table.name).await?;
        let has_time = catalog.iter().any(|column| column.name == "time");
        let sql = page_statement(schema, &table.name, query, has_time);
        let (records, has_more) = query_records(
            &endpoint,
            QueryLanguage::Sql,
            database,
            &sql,
            query.limit as usize,
        )
        .await?;
        let result_columns = catalog
            .into_iter()
            .map(|column| ResultColumn {
                timestamp: is_timestamp_type(&column.data_type),
                type_name: Some(column.data_type),
                name: column.name,
            })
            .collect();
        Ok(build_batch(&records, result_columns, has_more))
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        let endpoint = self.endpoint().await?;
        let database = self.database_or(table.database.as_deref())?;
        let schema = table.schema.as_deref().unwrap_or(MEASUREMENT_SCHEMA);
        let sql = count_statement(schema, &table.name, filter);
        let (records, _) = query_records(&endpoint, QueryLanguage::Sql, database, &sql, 1).await?;
        let count = records
            .first()
            .and_then(|record| record.get("count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        Ok(count)
    }

    async fn apply_changes(&self, _changes: &ChangeSet) -> Result<ChangeReport, DriverError> {
        Err(DriverError::unsupported(
            "InfluxDB tables are read-only here: points are written with line protocol \
             and cannot be updated or deleted individually",
        ))
    }
}
