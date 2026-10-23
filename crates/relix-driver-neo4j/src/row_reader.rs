/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::errors::map_error;
use crate::values::{to_value, type_label};
use async_trait::async_trait;
use neo4rs::{BoltType, DetachedRowStream, Row, RowStream, Txn};
use relix_core::{ColumnMeta, RowBatch, Value};
use relix_driver_api::DriverError;

#[async_trait]
pub(crate) trait RowSource: Send {
    async fn next_row(&mut self) -> Result<Option<Row>, neo4rs::Error>;

    async fn discard(self) -> Result<(), neo4rs::Error>;
}

#[async_trait]
impl RowSource for DetachedRowStream {
    async fn next_row(&mut self) -> Result<Option<Row>, neo4rs::Error> {
        self.next().await
    }

    async fn discard(self) -> Result<(), neo4rs::Error> {
        self.finish().await
    }
}

pub(crate) struct TransactionRows<'a> {
    pub(crate) stream: RowStream,
    pub(crate) txn: &'a mut Txn,
}

#[async_trait]
impl RowSource for TransactionRows<'_> {
    async fn next_row(&mut self) -> Result<Option<Row>, neo4rs::Error> {
        self.stream.next(&mut *self.txn).await
    }

    async fn discard(self) -> Result<(), neo4rs::Error> {
        self.stream.finish(self.txn).await
    }
}

/// Reads at most `limit` rows plus one look-ahead row that only sets `has_more`; the
/// rest of the result is discarded on the server instead of being transferred.
pub(crate) async fn read_rows(
    mut source: impl RowSource,
    limit: usize,
) -> Result<(Vec<Row>, bool), neo4rs::Error> {
    let mut rows = Vec::new();
    while let Some(row) = source.next_row().await? {
        if rows.len() == limit {
            source.discard().await?;
            return Ok((rows, true));
        }
        rows.push(row);
    }
    Ok((rows, false))
}

pub(crate) fn bolt_value(row: &Row, key: &str) -> Result<BoltType, DriverError> {
    row.get::<BoltType>(key)
        .map_err(|error| map_error(neo4rs::Error::DeserializationError(error)))
}

pub(crate) fn optional_text(row: &Row, key: &str) -> Result<Option<String>, DriverError> {
    Ok(match bolt_value(row, key)? {
        BoltType::String(text) => Some(text.value),
        _ => None,
    })
}

pub(crate) fn optional_integer(row: &Row, key: &str) -> Result<Option<i64>, DriverError> {
    Ok(match bolt_value(row, key)? {
        BoltType::Integer(number) => Some(number.value),
        _ => None,
    })
}

pub(crate) fn optional_bool(row: &Row, key: &str) -> Result<Option<bool>, DriverError> {
    Ok(match bolt_value(row, key)? {
        BoltType::Boolean(flag) => Some(flag.value),
        _ => None,
    })
}

pub(crate) fn text_list(row: &Row, key: &str) -> Result<Vec<String>, DriverError> {
    Ok(match bolt_value(row, key)? {
        BoltType::List(list) => list
            .value
            .into_iter()
            .filter_map(|item| match item {
                BoltType::String(text) => Some(text.value),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    })
}

pub(crate) fn row_keys(row: &Row) -> Vec<String> {
    row.keys()
        .into_iter()
        .map(|key| key.value.clone())
        .collect()
}

pub(crate) fn batch_from_rows(
    rows: &[Row],
    keys: Vec<String>,
    has_more: bool,
) -> Result<RowBatch, DriverError> {
    let mut type_names: Vec<Option<String>> = vec![None; keys.len()];
    let mut values = Vec::with_capacity(rows.len());
    for row in rows {
        let mut converted: Vec<Value> = Vec::with_capacity(keys.len());
        for (index, key) in keys.iter().enumerate() {
            let value = bolt_value(row, key)?;
            if type_names[index].is_none() && !matches!(value, BoltType::Null(_)) {
                type_names[index] = Some(type_label(&value).to_owned());
            }
            converted.push(to_value(&value));
        }
        values.push(converted);
    }
    Ok(RowBatch {
        columns: keys
            .into_iter()
            .zip(type_names)
            .map(|(name, type_name)| ColumnMeta::new(name, type_name))
            .collect(),
        rows: values,
        has_more,
    })
}
