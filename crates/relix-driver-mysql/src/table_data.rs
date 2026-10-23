/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::changes::apply_change_set;
use crate::connection::{MySqlConnection, sql_builder};
use crate::errors::map_error;
use crate::result_rows::query_rows;
use async_trait::async_trait;
use relix_core::{ChangeReport, ChangeSet, RowBatch, TableQuery, TableRef, Value};
use relix_driver_api::{DriverError, TableDataAccess};

#[async_trait]
impl TableDataAccess for MySqlConnection {
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        let sql = sql_builder().select_page(table, query);
        let limit = usize::try_from(query.limit).unwrap_or(usize::MAX);
        let mut conn = self.pooled().await?;
        query_rows(&mut conn, &sql, limit).await.map_err(map_error)
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        let sql = sql_builder().count(table, filter);
        let mut conn = self.pooled().await?;
        let batch = query_rows(&mut conn, &sql, 1).await.map_err(map_error)?;
        batch
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
        let mut conn = self.pooled().await?;
        apply_change_set(&mut conn, changes).await
    }
}

fn count_value(value: &Value) -> Option<u64> {
    match value {
        Value::UInt(count) => Some(*count),
        Value::Int(count) => u64::try_from(*count).ok(),
        Value::Text(text) => text.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_read_from_numeric_or_text_cells() {
        assert_eq!(count_value(&Value::Int(4)), Some(4));
        assert_eq!(count_value(&Value::UInt(5)), Some(5));
        assert_eq!(count_value(&Value::Text("6".into())), Some(6));
        assert_eq!(count_value(&Value::Int(-1)), None);
        assert_eq!(count_value(&Value::Null), None);
    }
}
