/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::OracleConnection;
use crate::errors::map_statement_error;
use crate::result_rows::read_rows;
use crate::statement_text::{executable_text, first_keyword};
use async_trait::async_trait;
use oracle::Connection;
use relix_core::StatementOutcome;
use relix_driver_api::{DriverError, ExecuteOptions, QueryExecutor};

const MAX_FETCH_ARRAY_SIZE: usize = 1000;

#[async_trait]
impl QueryExecutor for OracleConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let statement = statement.to_owned();
        let row_limit = options.row_limit;
        self.session
            .run(move |connection| {
                let text = executable_text(&statement);
                run_statement(connection, text, row_limit)
                    .map_err(|error| map_statement_error(error, text))
            })
            .await
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        self.session.cancel().await
    }
}

/// Runs one statement. Queries return their rows, PL/SQL blocks the first implicit result
/// (`DBMS_SQL.RETURN_RESULT`) if any, DML its row count. Oracle starts transactions
/// implicitly, so DML stays uncommitted until `COMMIT` (DDL commits on its own).
fn run_statement(
    connection: &Connection,
    text: &str,
    row_limit: usize,
) -> Result<StatementOutcome, oracle::Error> {
    let fetch_size = u32::try_from(row_limit.clamp(1, MAX_FETCH_ARRAY_SIZE) + 1).unwrap_or(1);
    let mut statement = connection
        .statement(text)
        .fetch_array_size(fetch_size)
        .build()?;
    if statement.is_query() {
        let rows = statement.query(&[])?;
        return Ok(StatementOutcome::Rows(read_rows(rows, row_limit)?));
    }
    statement.execute(&[])?;
    if statement.is_plsql()
        && let Some(mut cursor) = statement.implicit_result()?
    {
        return Ok(StatementOutcome::Rows(read_rows(
            cursor.query()?,
            row_limit,
        )?));
    }
    if statement.is_dml() {
        return Ok(StatementOutcome::Affected {
            rows: statement.row_count()?,
        });
    }
    Ok(StatementOutcome::Completed {
        message: match first_keyword(text) {
            Some(keyword) => format!("{keyword} completed"),
            None => "statement completed".to_owned(),
        },
    })
}
