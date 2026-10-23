/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::column_type::column_type_name;
use crate::row_count::ROW_COUNT_COLUMN;
use crate::values::from_column_data;
use futures_util::TryStreamExt;
use relix_core::{ColumnMeta, RowBatch, Value};
use tiberius::error::Error;
use tiberius::{Column, QueryItem, QueryStream, Row};

#[derive(Debug, Default, PartialEq)]
pub struct BatchResults {
    pub first: Option<RowBatch>,
    pub row_count: Option<u64>,
}

enum CurrentSet {
    First,
    RowCount,
    Skipped,
}

/// Reads all result sets of a batch. Rows are kept only for the first set (at most
/// `limit`, the rest is consumed from the wire and only marks the set as truncated) and
/// for the row-count marker appended by [`crate::row_count::with_row_count`]; further
/// sets are drained, so the connection stays usable.
pub async fn read_batch_results(
    mut stream: QueryStream<'_>,
    limit: usize,
) -> Result<BatchResults, Error> {
    let mut results = BatchResults::default();
    let mut current = CurrentSet::Skipped;
    while let Some(item) = stream.try_next().await? {
        match item {
            QueryItem::Metadata(metadata) => {
                current = results.start_set(metadata.columns());
            }
            QueryItem::Row(row) => results.add_row(&current, row, limit),
        }
    }
    Ok(results)
}

pub async fn read_first_result_set(
    stream: QueryStream<'_>,
    limit: usize,
) -> Result<RowBatch, Error> {
    Ok(read_batch_results(stream, limit)
        .await?
        .first
        .unwrap_or_default())
}

impl BatchResults {
    fn start_set(&mut self, columns: &[Column]) -> CurrentSet {
        if is_row_count_marker(columns) {
            return CurrentSet::RowCount;
        }
        if self.first.is_some() {
            return CurrentSet::Skipped;
        }
        self.first = Some(RowBatch {
            columns: column_metadata(columns),
            rows: Vec::new(),
            has_more: false,
        });
        CurrentSet::First
    }

    fn add_row(&mut self, current: &CurrentSet, row: Row, limit: usize) {
        match (current, self.first.as_mut()) {
            (CurrentSet::First, Some(batch)) if batch.rows.len() < limit => {
                batch.rows.push(convert_row(row));
            }
            (CurrentSet::First, Some(batch)) => batch.has_more = true,
            (CurrentSet::RowCount, _) => {
                self.row_count = row
                    .into_iter()
                    .next()
                    .map(from_column_data)
                    .and_then(|value| match value {
                        Value::Int(count) => u64::try_from(count).ok(),
                        _ => None,
                    });
            }
            _ => {}
        }
    }
}

fn is_row_count_marker(columns: &[Column]) -> bool {
    matches!(columns, [column] if column.name() == ROW_COUNT_COLUMN)
}

fn convert_row(row: Row) -> Vec<Value> {
    row.into_iter().map(from_column_data).collect()
}

fn column_metadata(columns: &[Column]) -> Vec<ColumnMeta> {
    columns
        .iter()
        .map(|column| {
            ColumnMeta::new(
                column.name(),
                Some(column_type_name(column.column_type()).to_owned()),
            )
        })
        .collect()
}
