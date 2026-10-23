/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::MsSqlConnection;
use crate::connector::MsSqlClient;
use crate::errors::map_error;
use crate::result_sets::{BatchResults, read_batch_results};
use crate::row_count::{modifies_rows, with_row_count};
use async_trait::async_trait;
use relix_core::{Dialect, StatementOutcome};
use relix_driver_api::{DriverError, ExecuteOptions, QueryExecutor};
use relix_query::top_level_words;
use tiberius::error::Error;

const KILL_NOT_PERMITTED: u32 = 6102;

#[async_trait]
impl QueryExecutor for MsSqlConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let mut slot = self.session.lock().await;
        let client = self.session.client(&mut slot).await?;
        let result = {
            let _running = self.session.start_statement();
            run_statement(client, statement, options.row_limit).await
        };
        result.map_err(|error| self.session.statement_failed(&mut slot, error, statement))
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        let Some(process_id) = self.session.request_kill() else {
            return Ok(());
        };
        let result = self.kill_session(process_id).await;
        if result.is_err() {
            self.session.kill_failed();
        }
        result
    }
}

impl MsSqlConnection {
    async fn kill_session(&self, process_id: i32) -> Result<(), DriverError> {
        let mut lease = self.data.lease().await?;
        let killed = kill(lease.client()?, process_id).await;
        lease.finish(killed)
    }
}

async fn run_statement(
    client: &mut MsSqlClient,
    statement: &str,
    row_limit: usize,
) -> Result<StatementOutcome, Error> {
    let batch = if modifies_rows(statement) {
        with_row_count(statement)
    } else {
        statement.to_owned()
    };
    let stream = client.simple_query(batch).await?;
    let results = read_batch_results(stream, row_limit).await?;
    Ok(outcome(statement, results))
}

fn outcome(statement: &str, results: BatchResults) -> StatementOutcome {
    match (results.first, results.row_count) {
        (Some(batch), _) => StatementOutcome::Rows(batch),
        (None, Some(rows)) => StatementOutcome::Affected { rows },
        (None, None) => StatementOutcome::Completed {
            message: match top_level_words(statement, Dialect::MsSql).first() {
                Some(keyword) => format!("{keyword} completed"),
                None => "statement completed".to_owned(),
            },
        },
    }
}

async fn kill(client: &mut MsSqlClient, process_id: i32) -> Result<(), DriverError> {
    let killed = match client.simple_query(format!("KILL {process_id}")).await {
        Ok(stream) => stream.into_results().await.map(drop),
        Err(error) => Err(error),
    };
    killed.map_err(|error| match error {
        Error::Server(token) if token.code() == KILL_NOT_PERMITTED => DriverError::unsupported(
            "cancelling a statement needs the ALTER ANY CONNECTION permission, \
             because SQL Server sessions are cancelled with KILL",
        )
        .with_detail(token.message().to_owned()),
        other => map_error(other),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::RowBatch;

    #[test]
    fn row_sets_win_over_row_counts() {
        let rows = BatchResults {
            first: Some(RowBatch::default()),
            row_count: Some(2),
        };
        assert!(matches!(
            outcome("UPDATE t SET a = 1 OUTPUT inserted.a", rows),
            StatementOutcome::Rows(_)
        ));
        let affected = BatchResults {
            row_count: Some(2),
            ..BatchResults::default()
        };
        assert_eq!(
            outcome("UPDATE t SET a = 1", affected),
            StatementOutcome::Affected { rows: 2 }
        );
        assert_eq!(
            outcome("/* x */ create table t (a int)", BatchResults::default()),
            StatementOutcome::Completed {
                message: "CREATE completed".into()
            }
        );
    }
}
