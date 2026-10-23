/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::ClickHouseConnection;
use async_trait::async_trait;
use relix_core::{Dialect, StatementOutcome};
use relix_driver_api::{DriverError, ExecuteOptions, QueryExecutor};
use relix_query::{completion_message, top_level_words};

#[async_trait]
impl QueryExecutor for ClickHouseConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let words = top_level_words(statement, Dialect::ClickHouse);
        if words.words.is_empty() {
            return Err(DriverError::query("the statement is empty"));
        }
        if words.has_multiple_statements {
            return Err(DriverError::query(
                "only one statement can be executed at a time",
            ));
        }
        let outcome = self
            .session
            .run(&self.client, statement, options.row_limit)
            .await?;
        Ok(match outcome.batch {
            Some(batch) => StatementOutcome::Rows(batch),
            None if words.first() == Some("INSERT") => StatementOutcome::Affected {
                rows: outcome.written_rows,
            },
            None => StatementOutcome::Completed {
                message: completion_message(&words),
            },
        })
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        self.session.cancel(&self.client).await
    }
}
