/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::TrinoConnection;
use crate::statement_runner::run_statement;
use async_trait::async_trait;
use relix_core::{Dialect, StatementOutcome};
use relix_driver_api::{DriverError, ExecuteOptions, QueryExecutor};
use relix_query::{completion_message, top_level_words};

#[async_trait]
impl QueryExecutor for TrinoConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let words = top_level_words(statement, Dialect::Trino);
        if words.words.is_empty() {
            return Err(DriverError::query("the statement is empty"));
        }
        if words.has_multiple_statements {
            return Err(DriverError::query(
                "only one statement can be executed at a time",
            ));
        }
        let sql = statement.trim_end().trim_end_matches(';');
        let mut session = self.session.lock().await;
        let outcome = run_statement(
            &self.client,
            sql,
            &mut session,
            options.row_limit,
            Some(&self.running),
        )
        .await?;
        Ok(
            match (outcome.update_type, outcome.update_count, outcome.batch) {
                (Some(_), Some(rows), _) => StatementOutcome::Affected { rows },
                (Some(update_type), None, _) => StatementOutcome::Completed {
                    message: update_type,
                },
                (None, _, Some(batch)) => StatementOutcome::Rows(batch),
                (None, _, None) => StatementOutcome::Completed {
                    message: completion_message(&words),
                },
            },
        )
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        match self.running.current() {
            Some(next_uri) => self.client.abandon(&next_uri).await,
            None => Ok(()),
        }
    }
}
