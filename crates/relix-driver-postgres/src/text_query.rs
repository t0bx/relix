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

#[derive(Debug, Default)]
pub(crate) struct Description {
    column_types: Option<Vec<String>>,
    prepare_error: Option<tokio_postgres::Error>,
}

/// Runs SQL through the simple query protocol so any statement is accepted, keeping at
/// most `row_limit` rows of the last result set and draining the remainder without
/// buffering it. With `describe`, column types are learned first (see [`describe`]).
pub(crate) async fn run_text_query(
    client: &Client,
    sql: &str,
    row_limit: usize,
    describe_columns: bool,
) -> Result<TextQueryResult, tokio_postgres::Error> {
    let description = if describe_columns {
        describe(client, sql).await?
    } else {
        Description::default()
    };
    run_described(client, sql, row_limit, description).await
}

/// Prepares the statement to learn its column types for typed values. A failed prepare
/// is kept so the statement still runs; only a cancel request that interrupted the
/// prepare step fails the whole statement.
pub(crate) async fn describe(
    client: &Client,
    sql: &str,
) -> Result<Description, tokio_postgres::Error> {
    match client.prepare(sql).await {
        Ok(statement) => Ok(Description {
            column_types: Some(
                statement
                    .columns()
                    .iter()
                    .map(|column| column.type_().name().to_owned())
                    .collect(),
            ),
            prepare_error: None,
        }),
        Err(error) if error.code() == Some(&SqlState::QUERY_CANCELED) => Err(error),
        Err(error) => Ok(Description {
            column_types: None,
            prepare_error: Some(error),
        }),
    }
}

/// Runs a statement described by [`describe`]; if preparing it aborted the surrounding
/// transaction, the original prepare error is reported instead of the abort notice.
pub(crate) async fn run_described(
    client: &Client,
    sql: &str,
    row_limit: usize,
    description: Description,
) -> Result<TextQueryResult, tokio_postgres::Error> {
    let Description {
        column_types,
        prepare_error,
    } = description;
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
