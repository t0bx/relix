/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::values::typed_value;
use futures_util::StreamExt;
use relix_core::{ColumnMeta, RowBatch, Value};
use std::sync::Arc;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, SimpleColumn, SimpleQueryMessage, SimpleQueryRow};

#[derive(Debug, Default)]
pub(crate) struct TextQueryResult {
    pub(crate) batch: Option<RowBatch>,
    pub(crate) affected_rows: u64,
}

/// Runs SQL through the simple query protocol so any statement is accepted, keeping at
/// most `row_limit` rows of the last result set and draining the remainder without
/// buffering it. With `describe`, the statement is prepared first to learn column types
/// for typed values; when preparing fails the statement still runs, and if preparing
/// aborted the surrounding transaction the original prepare error is reported instead.
pub(crate) async fn run_text_query(
    client: &Client,
    sql: &str,
    row_limit: usize,
    describe: bool,
) -> Result<TextQueryResult, tokio_postgres::Error> {
    let (column_types, prepare_error) = if describe {
        match client.prepare(sql).await {
            Ok(statement) => (
                Some(
                    statement
                        .columns()
                        .iter()
                        .map(|column| column.type_().name().to_owned())
                        .collect::<Vec<_>>(),
                ),
                None,
            ),
            Err(error) => (None, Some(error)),
        }
    } else {
        (None, None)
    };
    match stream_rows(client, sql, row_limit, column_types.as_deref()).await {
        Err(error) if aborted_by_prepare(&error, prepare_error.as_ref()) => {
            Err(prepare_error.unwrap_or(error))
        }
        other => other,
    }
}

async fn stream_rows(
    client: &Client,
    sql: &str,
    row_limit: usize,
    column_types: Option<&[String]>,
) -> Result<TextQueryResult, tokio_postgres::Error> {
    let stream = client.simple_query_raw(sql).await?;
    let mut stream = std::pin::pin!(stream);
    let mut result = TextQueryResult::default();
    let mut types: Vec<Option<String>> = Vec::new();
    while let Some(message) = stream.next().await {
        match message? {
            SimpleQueryMessage::RowDescription(columns) => {
                types = column_type_names(&columns, column_types);
                result.batch = Some(empty_batch(&columns, &types));
            }
            SimpleQueryMessage::Row(row) => {
                if let Some(batch) = result.batch.as_mut() {
                    if batch.rows.len() < row_limit {
                        batch.rows.push(row_values(&row, &types)?);
                    } else {
                        batch.has_more = true;
                    }
                }
            }
            SimpleQueryMessage::CommandComplete(rows) => result.affected_rows = rows,
            _ => {}
        }
    }
    Ok(result)
}

fn column_type_names(
    columns: &Arc<[SimpleColumn]>,
    described: Option<&[String]>,
) -> Vec<Option<String>> {
    match described {
        Some(types) if types.len() == columns.len() => types.iter().cloned().map(Some).collect(),
        _ => vec![None; columns.len()],
    }
}

fn empty_batch(columns: &Arc<[SimpleColumn]>, types: &[Option<String>]) -> RowBatch {
    RowBatch {
        columns: columns
            .iter()
            .zip(types)
            .map(|(column, type_name)| ColumnMeta::new(column.name(), type_name.clone()))
            .collect(),
        rows: Vec::new(),
        has_more: false,
    }
}

fn row_values(
    row: &SimpleQueryRow,
    types: &[Option<String>],
) -> Result<Vec<Value>, tokio_postgres::Error> {
    (0..row.len())
        .map(|index| {
            let type_name = types.get(index).and_then(Option::as_deref);
            Ok(typed_value(type_name, row.try_get(index)?))
        })
        .collect()
}

fn aborted_by_prepare(
    error: &tokio_postgres::Error,
    prepare_error: Option<&tokio_postgres::Error>,
) -> bool {
    let Some(prepare_error) = prepare_error else {
        return false;
    };
    error.code() == Some(&SqlState::IN_FAILED_SQL_TRANSACTION)
        && prepare_error.as_db_error().is_some()
        && prepare_error.code() != Some(&SqlState::IN_FAILED_SQL_TRANSACTION)
}
