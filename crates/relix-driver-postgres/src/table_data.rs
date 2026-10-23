/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::PostgresConnection;
use crate::errors::map_error;
use crate::text_query::run_text_query;
use async_trait::async_trait;
use relix_core::{ChangeReport, ChangeSet, RowBatch, RowChange, TableQuery, TableRef, Value};
use relix_driver_api::{DriverError, ErrorKind, TableDataAccess};
use tokio_postgres::Transaction;

#[async_trait]
impl TableDataAccess for PostgresConnection {
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        let sql = self.builder.select_page(table, query);
        let client = self.data_client(table.database.as_deref()).await?;
        let guard = client.read().await;
        let result = run_text_query(&guard, &sql, query.limit as usize, true)
            .await
            .map_err(map_error)?;
        result
            .batch
            .ok_or_else(|| DriverError::internal("page query returned no result set"))
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        let sql = self.builder.count(table, filter);
        let client = self.data_client(table.database.as_deref()).await?;
        let guard = client.read().await;
        let result = run_text_query(&guard, &sql, 1, false)
            .await
            .map_err(map_error)?;
        let count = result
            .batch
            .and_then(|batch| batch.rows.into_iter().next())
            .and_then(|row| row.into_iter().next());
        match count {
            Some(Value::Text(text)) => text
                .parse()
                .map_err(|_| DriverError::internal(format!("invalid row count '{text}'"))),
            _ => Err(DriverError::internal("count query returned no value")),
        }
    }

    async fn apply_changes(&self, changes: &ChangeSet) -> Result<ChangeReport, DriverError> {
        if changes.is_empty() {
            return Ok(ChangeReport::default());
        }
        let client = self.data_client(changes.table.database.as_deref()).await?;
        let mut guard = client.write().await;
        let transaction = guard.transaction().await.map_err(map_error)?;
        match self.apply_in_transaction(&transaction, changes).await {
            Ok(report) => {
                transaction.commit().await.map_err(map_error)?;
                Ok(report)
            }
            Err(error) => {
                let _ = transaction.rollback().await;
                Err(error)
            }
        }
    }
}

impl PostgresConnection {
    async fn apply_in_transaction(
        &self,
        transaction: &Transaction<'_>,
        changes: &ChangeSet,
    ) -> Result<ChangeReport, DriverError> {
        let mut report = ChangeReport::default();
        for (index, change) in changes.changes.iter().enumerate() {
            let statement = self.builder.change_statement(&changes.table, change)?;
            let affected = transaction
                .execute(statement.sql.as_str(), &[])
                .await
                .map_err(map_error)?;
            match change {
                RowChange::Insert { .. } => report.inserted += affected,
                RowChange::Update { .. } => {
                    ensure_single_row(index, "update", affected)?;
                    report.updated += 1;
                }
                RowChange::Delete { .. } => {
                    ensure_single_row(index, "delete", affected)?;
                    report.deleted += 1;
                }
            }
        }
        Ok(report)
    }
}

fn ensure_single_row(index: usize, operation: &str, affected: u64) -> Result<(), DriverError> {
    if affected == 1 {
        return Ok(());
    }
    Err(DriverError::new(
        ErrorKind::ConcurrentModification,
        format!(
            "change #{} ({operation}) affected {affected} rows instead of exactly 1; \
             the row was modified or removed by someone else, nothing was saved",
            index + 1
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_row_changes_pass_and_others_conflict() {
        assert!(ensure_single_row(0, "update", 1).is_ok());
        let error = ensure_single_row(2, "delete", 0).unwrap_err();
        assert_eq!(error.kind, ErrorKind::ConcurrentModification);
        assert!(
            error
                .message
                .starts_with("change #3 (delete) affected 0 rows")
        );
    }
}
