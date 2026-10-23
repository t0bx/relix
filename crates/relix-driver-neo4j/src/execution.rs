/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::connection::Neo4jConnection;
use crate::cypher::open_stream;
use crate::errors::{map_error, map_statement_error};
use crate::projection::{order_keys, projection_columns};
use crate::row_reader::{TransactionRows, batch_from_rows, read_rows, row_keys};
use crate::termination::{tagged, terminate_session_transactions};
use crate::transaction_control::TransactionCommand;
use async_trait::async_trait;
use neo4rs::{Row, query};
use relix_core::{ColumnMeta, RowBatch, StatementOutcome};
use relix_driver_api::{DriverError, ExecuteOptions, QueryExecutor};

const ROLLED_BACK_NOTE: &str = "the open transaction was rolled back";

#[async_trait]
impl QueryExecutor for Neo4jConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let cypher = statement.trim().trim_end_matches(';').trim_end();
        if cypher.is_empty() {
            return Err(DriverError::query("empty statement"));
        }
        if let Some(command) = TransactionCommand::parse(cypher) {
            return self.control_transaction(command).await;
        }
        let live = self.live().await?;
        let mut transaction = self.transaction.lock().await;
        let limit = options.row_limit;
        let result = self
            .cancel_signal
            .run(async {
                let read = match transaction.as_mut() {
                    Some(txn) => match txn.execute(query(cypher)).await {
                        Ok(stream) => read_rows(TransactionRows { stream, txn }, limit).await,
                        Err(error) => Err(error),
                    },
                    None => {
                        match open_stream(&live.graph, None, tagged(cypher, &self.session_token))
                            .await
                        {
                            Ok(stream) => read_rows(stream, limit).await,
                            Err(error) => Err(error),
                        }
                    }
                };
                read.map_err(map_statement_error)
            })
            .await;
        let (rows, has_more) = match result {
            Ok(read) => read,
            Err(error) => match transaction.take() {
                Some(open) => {
                    let _ = open.rollback().await;
                    return Err(with_note(error, ROLLED_BACK_NOTE));
                }
                None => return Err(error),
            },
        };
        statement_outcome(&rows, has_more, cypher)
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        let live = self.live().await?;
        self.cancel_signal.cancel();
        terminate_session_transactions(&live.graph, &self.session_token).await?;
        Ok(())
    }
}

impl Neo4jConnection {
    async fn control_transaction(
        &self,
        command: TransactionCommand,
    ) -> Result<StatementOutcome, DriverError> {
        let live = self.live().await?;
        let mut transaction = self.transaction.lock().await;
        let message = match command {
            TransactionCommand::Begin => {
                if transaction.is_some() {
                    return Err(DriverError::query("a transaction is already open"));
                }
                *transaction = Some(live.graph.start_txn().await.map_err(map_error)?);
                "transaction started"
            }
            TransactionCommand::Commit => {
                let open = transaction.take().ok_or_else(no_transaction)?;
                open.commit().await.map_err(map_error)?;
                "transaction committed"
            }
            TransactionCommand::Rollback => {
                let open = transaction.take().ok_or_else(no_transaction)?;
                open.rollback().await.map_err(map_error)?;
                "transaction rolled back"
            }
        };
        Ok(StatementOutcome::Completed {
            message: message.to_owned(),
        })
    }
}

/// Builds the console result. Without rows the result columns cannot be read from
/// neo4rs, so they are taken from the statement's final projection; statements without
/// a projection (pure writes, schema commands) complete without a result set.
fn statement_outcome(
    rows: &[Row],
    has_more: bool,
    cypher: &str,
) -> Result<StatementOutcome, DriverError> {
    let Some(first) = rows.first() else {
        return Ok(match projection_columns(cypher) {
            Some(columns) => StatementOutcome::Rows(RowBatch {
                columns: columns
                    .into_iter()
                    .map(|name| ColumnMeta::new(name, None))
                    .collect(),
                rows: Vec::new(),
                has_more: false,
            }),
            None => StatementOutcome::Completed {
                message: "statement executed".to_owned(),
            },
        });
    };
    let keys = order_keys(row_keys(first), cypher);
    batch_from_rows(rows, keys, has_more).map(StatementOutcome::Rows)
}

fn no_transaction() -> DriverError {
    DriverError::query("no transaction is open; start one with BEGIN")
}

fn with_note(error: DriverError, note: &str) -> DriverError {
    let detail = match &error.detail {
        Some(detail) => format!("{detail}; {note}"),
        None => note.to_owned(),
    };
    error.with_detail(detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_without_rows_use_the_projection() {
        let outcome =
            statement_outcome(&[], false, "MATCH (n:Missing) RETURN n.name AS name").unwrap();
        match outcome {
            StatementOutcome::Rows(batch) => {
                assert_eq!(batch.columns[0].name, "name");
                assert!(batch.rows.is_empty());
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            statement_outcome(&[], false, "CREATE (:Thing)").unwrap(),
            StatementOutcome::Completed {
                message: "statement executed".into()
            }
        );
    }

    #[test]
    fn notes_are_appended_to_existing_details() {
        let error = with_note(DriverError::query("x").with_detail("code"), "rolled back");
        assert_eq!(error.detail.as_deref(), Some("code; rolled back"));
    }
}
