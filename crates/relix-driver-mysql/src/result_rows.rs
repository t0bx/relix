/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::column_type::type_name;
use crate::values::from_mysql;
use mysql_async::prelude::Queryable;
use mysql_async::{Column, Conn, Error, QueryResult, Row, TextProtocol};
use relix_core::{ColumnMeta, RowBatch, Value};
use std::sync::Arc;

/// Reads the current result set, keeping at most `limit` converted rows. Remaining rows are
/// still consumed from the wire (so the connection stays usable) but are discarded.
pub async fn read_rows(
    result: &mut QueryResult<'_, '_, TextProtocol>,
    limit: usize,
) -> Result<RowBatch, Error> {
    let columns: Arc<[Column]> = result.columns().unwrap_or_else(|| Arc::from(Vec::new()));
    let mut rows = Vec::new();
    let mut has_more = false;
    result
        .for_each(|row| {
            if rows.len() < limit {
                rows.push(convert_row(&columns, row));
            } else {
                has_more = true;
            }
        })
        .await?;
    Ok(RowBatch {
        columns: column_metadata(&columns),
        rows,
        has_more,
    })
}

pub async fn query_rows(conn: &mut Conn, sql: &str, limit: usize) -> Result<RowBatch, Error> {
    let mut result = conn.query_iter(sql).await?;
    let batch = read_rows(&mut result, limit).await?;
    result.drop_result().await?;
    Ok(batch)
}

pub fn has_columns(result: &QueryResult<'_, '_, TextProtocol>) -> bool {
    !result.columns_ref().is_empty()
}

fn convert_row(columns: &[Column], row: Row) -> Vec<Value> {
    row.unwrap_raw()
        .into_iter()
        .zip(columns)
        .map(|(value, column)| value.map_or(Value::Null, |value| from_mysql(column, value)))
        .collect()
}

fn column_metadata(columns: &[Column]) -> Vec<ColumnMeta> {
    columns
        .iter()
        .map(|column| ColumnMeta::new(column.name_str(), Some(type_name(column))))
        .collect()
}
