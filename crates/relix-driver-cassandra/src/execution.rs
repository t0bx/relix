/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::CassandraConnection;
use crate::paged_query::{RowWindow, run_paged};
use async_trait::async_trait;
use relix_core::StatementOutcome;
use relix_driver_api::{DriverError, ExecuteOptions, QueryExecutor};

#[async_trait]
impl QueryExecutor for CassandraConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let cql = statement.trim().trim_end_matches(';').trim_end();
        if cql.is_empty() {
            return Err(DriverError::query("empty statement"));
        }
        let live = self.live().await?;
        let window = RowWindow {
            skip: 0,
            take: options.row_limit,
        };
        let batch = self
            .cancel_signal
            .run(run_paged(&live.session, cql, window, self.query_timeout))
            .await?;
        Ok(match batch {
            Some(batch) => StatementOutcome::Rows(batch),
            None => StatementOutcome::Completed {
                message: completion_message(cql),
            },
        })
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        self.live().await?;
        self.cancel_signal.cancel();
        Ok(())
    }
}

fn completion_message(cql: &str) -> String {
    let words: Vec<String> = cql
        .split_whitespace()
        .take(3)
        .map(str::to_uppercase)
        .collect();
    let command = match words.first().map(String::as_str) {
        Some("CREATE" | "ALTER" | "DROP") => match words.get(1).map(String::as_str) {
            Some("MATERIALIZED" | "CUSTOM") => words.join(" "),
            Some(_) => words[..2].join(" "),
            None => words.join(" "),
        },
        Some(first) => first.to_owned(),
        None => String::new(),
    };
    format!("{command} executed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_messages_name_the_command() {
        assert_eq!(
            completion_message("insert into t (a) values (1)"),
            "INSERT executed"
        );
        assert_eq!(
            completion_message("CREATE TABLE ks.t (a int PRIMARY KEY)"),
            "CREATE TABLE executed"
        );
        assert_eq!(
            completion_message("drop materialized view ks.v"),
            "DROP MATERIALIZED VIEW executed"
        );
        assert_eq!(completion_message("USE ks"), "USE executed");
    }
}
