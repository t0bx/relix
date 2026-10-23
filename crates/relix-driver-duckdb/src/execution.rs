/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::DuckDbConnection;
use crate::errors::map_statement_error;
use crate::rows::{ResultColumn, execute_streaming, first_cell, read_batch};
use crate::statement_plan::{StatementPlan, StatusResult};
use async_trait::async_trait;
use duckdb::Connection;
use relix_core::{StatementOutcome, Value};
use relix_driver_api::{DriverError, ExecuteOptions, QueryExecutor};

#[async_trait]
impl QueryExecutor for DuckDbConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let sql = statement.to_owned();
        let row_limit = options.row_limit;
        self.session
            .run(move |connection| execute_statement(connection, &sql, row_limit))
            .await
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        self.session.interrupt();
        Ok(())
    }
}

fn execute_statement(
    connection: &Connection,
    sql: &str,
    row_limit: usize,
) -> Result<StatementOutcome, DriverError> {
    let plan = StatementPlan::of(sql);
    if plan.is_empty() {
        return Err(DriverError::query("the statement is empty"));
    }
    if plan.has_multiple_statements() {
        return Err(DriverError::query(
            "only one statement can be executed at a time",
        ));
    }
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| map_statement_error(error, sql))?;
    let columns = execute_streaming(&mut statement, [], sql)?;
    match plan.status_result(&column_shape(&columns)) {
        Some(StatusResult::ChangedRows) => {
            let batch = read_batch(&statement, &columns, 1)?;
            Ok(StatementOutcome::Affected {
                rows: changed_rows(first_cell(&batch)),
            })
        }
        Some(StatusResult::Completed) => {
            read_batch(&statement, &columns, 1)?;
            Ok(StatementOutcome::Completed {
                message: plan.completion_message(),
            })
        }
        None => read_batch(&statement, &columns, row_limit).map(StatementOutcome::Rows),
    }
}

fn column_shape(columns: &[ResultColumn]) -> Vec<(String, String)> {
    columns
        .iter()
        .map(|column| (column.name.clone(), column.column_type.name.clone()))
        .collect()
}

fn changed_rows(cell: Option<&Value>) -> u64 {
    match cell {
        Some(Value::Int(count)) => u64::try_from(*count).unwrap_or_default(),
        Some(Value::UInt(count)) => *count,
        _ => 0,
    }
}
