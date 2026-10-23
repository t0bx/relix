/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::SnowflakeConnection;
use async_trait::async_trait;
use relix_core::{ChangeReport, ChangeSet, RowBatch, TableQuery, TableRef};
use relix_driver_api::{DriverError, ErrorKind, TableDataAccess};

#[async_trait]
impl TableDataAccess for SnowflakeConnection {
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        let limit = query.limit as usize;
        let sql = self.builder.select_page(table, query);
        let mut rows = self.query_rows(&sql, limit + 1).await?;
        rows.has_more = rows.rows.len() > limit;
        rows.rows.truncate(limit);
        Ok(rows.into_batch())
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        let count = self.scalar(&self.builder.count(table, filter)).await?;
        count
            .and_then(|count| count.trim().parse().ok())
            .ok_or_else(|| DriverError::new(ErrorKind::Protocol, "COUNT(*) returned no number"))
    }

    async fn apply_changes(&self, _changes: &ChangeSet) -> Result<ChangeReport, DriverError> {
        Err(
            DriverError::unsupported("Snowflake tables are read-only in the data grid")
                .with_detail(
                    "the SQL API runs every request in its own session and Snowflake does not \
                 enforce primary keys, so guarded single-row changes cannot be guaranteed; \
                 change data with SQL statements in the editor",
                ),
        )
    }
}
