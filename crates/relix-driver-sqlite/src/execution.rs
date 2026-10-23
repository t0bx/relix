/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::SqliteConnection;
use crate::errors::map_error;
use crate::rows::read_batch;
use crate::statement_kind::StatementKind;
use async_trait::async_trait;
use relix_core::StatementOutcome;
use relix_driver_api::{DriverError, ExecuteOptions, QueryExecutor};
use rusqlite::Connection;

#[async_trait]
impl QueryExecutor for SqliteConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let sql = statement.to_owned();
        let row_limit = options.row_limit;
        self.shared
            .run(move |connection| execute_statement(connection, &sql, row_limit))
            .await
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        self.shared.interrupt();
        Ok(())
    }
}

fn execute_statement(
    connection: &Connection,
    sql: &str,
    row_limit: usize,
) -> Result<StatementOutcome, DriverError> {
    let kind = StatementKind::of(sql);
    if kind.is_empty() {
        return Err(DriverError::query("the statement is empty"));
    }
    let mut statement = connection.prepare(sql).map_err(map_error)?;
    if statement.column_count() > 0 {
        return read_batch(&mut statement, [], row_limit).map(StatementOutcome::Rows);
    }
    let readonly = statement.readonly();
    statement.raw_execute().map_err(map_error)?;
    if kind.reports_affected_rows(readonly) {
        return Ok(StatementOutcome::Affected {
            rows: connection.changes(),
        });
    }
    Ok(StatementOutcome::Completed {
        message: kind.completion_message(),
    })
}
