/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::SnowflakeConnection;
use crate::result::statement_outcome;
use crate::transaction_control::reject_transaction_control;
use async_trait::async_trait;
use relix_core::{Dialect, StatementOutcome};
use relix_driver_api::{DriverError, ExecuteOptions, QueryExecutor};
use relix_query::classify;

#[async_trait]
impl QueryExecutor for SnowflakeConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        reject_transaction_control(statement)?;
        let kind = classify(statement, Dialect::Snowflake).kind;
        let _running = self.running.start();
        let first = self.submit(statement, Some(&self.running)).await?;
        let outcome = statement_outcome(self.client(), first, kind, options.row_limit).await?;
        if let Some(context) = self.context().after_use(statement) {
            self.replace_context(context);
        }
        Ok(outcome)
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        match self.running.request_cancel() {
            Some(handle) => self.client().cancel(&handle).await,
            None => Ok(()),
        }
    }
}
