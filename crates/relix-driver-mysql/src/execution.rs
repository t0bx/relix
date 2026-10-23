/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::MySqlConnection;
use crate::errors::{closed_connection, map_error, map_statement_error};
use crate::result_rows::{has_columns, read_rows};
use crate::statement_kind::outcome_without_rows;
use async_trait::async_trait;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Error};
use relix_core::StatementOutcome;
use relix_driver_api::{DriverError, ExecuteOptions, QueryExecutor};

#[async_trait]
impl QueryExecutor for MySqlConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let mut session = self.session.lock().await;
        let conn = session.as_mut().ok_or_else(closed_connection)?;
        run_statement(conn, statement, options.row_limit)
            .await
            .map_err(|error| map_statement_error(error, statement))
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        let mut conn = self.pooled().await?;
        conn.query_drop(format!("KILL QUERY {}", self.session_id))
            .await
            .map_err(map_error)
    }
}

async fn run_statement(
    conn: &mut Conn,
    statement: &str,
    row_limit: usize,
) -> Result<StatementOutcome, Error> {
    let mut result = conn.query_iter(statement).await?;
    let outcome = if has_columns(&result) {
        StatementOutcome::Rows(read_rows(&mut result, row_limit).await?)
    } else {
        outcome_without_rows(statement, result.affected_rows(), &result.info())
    };
    result.drop_result().await?;
    Ok(outcome)
}
