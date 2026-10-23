/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::jobs::cancel_job;
use crate::connection::BigQueryConnection;
use crate::result::{page_size, statement_outcome};
use async_trait::async_trait;
use relix_core::StatementOutcome;
use relix_driver_api::{DriverError, ExecuteOptions, QueryExecutor};

#[async_trait]
impl QueryExecutor for BigQueryConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let _running = self.running.start();
        let results = self
            .query_in_session(statement, page_size(options.row_limit))
            .await?;
        statement_outcome(self.client(), results, options.row_limit).await
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        match self.running.request_cancel() {
            Some(reference) => cancel_job(self.client(), &reference).await,
            None => Ok(()),
        }
    }
}
