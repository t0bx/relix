/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::command_tag::CommandInfo;
use crate::connection::PostgresConnection;
use crate::errors::map_statement_error;
use crate::statement_text::scan_statement;
use crate::text_query::run_text_query;
use async_trait::async_trait;
use relix_core::StatementOutcome;
use relix_driver_api::{DriverError, ExecuteOptions, SqlExecutor};

#[async_trait]
impl SqlExecutor for PostgresConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let scan = scan_statement(statement);
        let command = CommandInfo::from_scan(&scan);
        if command.streams_copy_data() {
            return Err(DriverError::unsupported(
                "COPY ... FROM STDIN / TO STDOUT cannot run in the editor; use a server-side file",
            ));
        }
        let describe = command.may_return_rows() && !scan.has_multiple_statements;
        let session = self.session().await?;
        let client = session.read().await;
        let result = run_text_query(&client, statement, options.row_limit, describe)
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
