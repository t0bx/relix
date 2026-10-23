/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::command_tag::CommandInfo;
use crate::connection::PostgresConnection;
use crate::errors::map_statement_error;
use crate::text_query::{Description, describe, run_described};
use async_trait::async_trait;
use relix_core::{Dialect, StatementOutcome};
use relix_driver_api::{DriverError, ExecuteOptions, QueryExecutor};
use relix_query::top_level_words;

#[async_trait]
impl QueryExecutor for PostgresConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let words = top_level_words(statement, Dialect::Postgres);
        let command = CommandInfo::from_words(&words);
        if command.streams_copy_data() {
            return Err(DriverError::unsupported(
                "COPY ... FROM STDIN / TO STDOUT cannot run in the editor; use a server-side file",
            ));
        }
        let describe_columns = command.may_return_rows() && !words.has_multiple_statements;
        let session = self.session().await?;
        let client = session.read().await;
        let _running = self.cancel_signal.start();
        let description = if describe_columns {
            describe(&client, statement)
                .await
                .map_err(map_statement_error)?
        } else {
            Description::default()
        };
        self.cancel_signal.ensure_not_requested()?;
        let result = run_described(&client, statement, options.row_limit, description)
            .await
            .map_err(map_statement_error)?;
        Ok(match result.batch {
            Some(batch) => StatementOutcome::Rows(batch),
            None => command.outcome(result.affected_rows),
        })
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        self.cancel_session().await
    }
}
