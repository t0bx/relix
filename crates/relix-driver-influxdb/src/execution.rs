/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::connection::InfluxDbConnection;
use crate::query_client::query_records;
use crate::result_batch::{build_batch, inferred_columns};
use async_trait::async_trait;
use relix_core::StatementOutcome;
use relix_driver_api::{DriverError, ExecuteOptions, QueryExecutor};

#[async_trait]
impl QueryExecutor for InfluxDbConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let query = statement.trim().trim_end_matches(';').trim_end();
        if query.is_empty() {
            return Err(DriverError::query("empty statement"));
        }
        let database = self.database_or(None)?;
        let endpoint = self.endpoint().await?;
        let (records, has_more) = self
            .cancel_signal
            .run(query_records(
                &endpoint,
                self.language,
                database,
                query,
                options.row_limit,
            ))
            .await?;
        let columns = inferred_columns(&records);
        Ok(StatementOutcome::Rows(build_batch(
            &records, columns, has_more,
        )))
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        self.endpoint().await?;
        self.cancel_signal.cancel();
        Ok(())
    }
}
