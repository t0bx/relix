/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::changes::apply_change_set;
use crate::connection::{MsSqlConnection, sql_builder};
use crate::connector::MsSqlClient;
use crate::errors::map_error;
use crate::result_sets::read_first_result_set;
use async_trait::async_trait;
use relix_core::{ChangeReport, ChangeSet, RowBatch, TableQuery, TableRef, Value};
use relix_driver_api::{DriverError, TableDataAccess};

#[async_trait]
impl TableDataAccess for MsSqlConnection {
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        let sql = sql_builder().select_page(table, query);
        let limit = usize::try_from(query.limit).unwrap_or(usize::MAX);
        let mut lease = self.data.lease().await?;
        let page = query_batch(lease.client()?, &sql, limit).await;
        lease.finish(page)
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        let sql = sql_builder().count(table, filter);
        let mut lease = self.data.lease().await?;
        let batch = query_batch(lease.client()?, &sql, 1).await;
        lease
            .finish(batch)?
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(count_value)
            .ok_or_else(|| DriverError::internal("COUNT(*) returned no numeric value"))
    }

    async fn apply_changes(&self, changes: &ChangeSet) -> Result<ChangeReport, DriverError> {
        if changes.is_empty() {
            return Ok(ChangeReport::default());
        }
        let mut lease = self.data.lease().await?;
        let report = apply_change_set(lease.client()?, changes).await;
        lease.finish(report)
    }
}

async fn query_batch(
    client: &mut MsSqlClient,
    sql: &str,
    limit: usize,
) -> Result<RowBatch, DriverError> {
    let stream = client.simple_query(sql).await.map_err(map_error)?;
    read_first_result_set(stream, limit)
        .await
        .map_err(map_error)
}

fn count_value(value: &Value) -> Option<u64> {
    match value {
        Value::Int(count) => u64::try_from(*count).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_non_negative_integers() {
        assert_eq!(count_value(&Value::Int(4)), Some(4));
        assert_eq!(count_value(&Value::Int(-1)), None);
        assert_eq!(count_value(&Value::Null), None);
    }
}
