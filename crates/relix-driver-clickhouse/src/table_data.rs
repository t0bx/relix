/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::QueryRequest;
use crate::connection::ClickHouseConnection;
use crate::metadata_rows::text_at;
use async_trait::async_trait;
use relix_core::{ChangeReport, ChangeSet, Dialect, RowBatch, TableQuery, TableRef};
use relix_driver_api::{DriverError, ParamStyle, SqlBuilder, TableDataAccess};

fn clickhouse_builder() -> SqlBuilder {
    SqlBuilder::new(Dialect::ClickHouse, ParamStyle::Inline)
}

#[async_trait]
impl TableDataAccess for ClickHouseConnection {
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        let sql = clickhouse_builder().select_page(table, query);
        let outcome = self
            .client
            .query(&QueryRequest::new(&sql), query.limit as usize)
            .await?;
        Ok(outcome.batch.unwrap_or_default())
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        let sql = clickhouse_builder().count(table, filter);
        let batch = self.client.rows(&QueryRequest::new(&sql)).await?;
        let count = batch
            .rows
            .first()
            .map(|row| text_at(row, 0))
            .unwrap_or_default();
        count
            .parse()
            .map_err(|_| DriverError::internal(format!("unexpected row count \"{count}\"")))
    }

    async fn apply_changes(&self, _changes: &ChangeSet) -> Result<ChangeReport, DriverError> {
        Err(DriverError::unsupported(
            "ClickHouse tables cannot be edited in the grid: UPDATE and DELETE run as \
             asynchronous mutations, so single-row checks and rollback are not possible",
        ))
    }
}
