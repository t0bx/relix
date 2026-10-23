/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::{map_error, map_statement_error};
use crate::result_schema::ensure_decodable;
use crate::type_names::ColumnType;
use crate::values::from_duckdb;
use duckdb::{Params, Statement};
use relix_core::{ColumnMeta, RowBatch, Value};
use relix_driver_api::DriverError;

pub(crate) struct ResultColumn {
    pub(crate) name: String,
    pub(crate) column_type: ColumnType,
}

/// Executes the prepared statement in streaming mode: DuckDB produces result chunks
/// only as they are fetched, so a truncated result never runs the query to the end.
pub(crate) fn execute_streaming<P: Params>(
    statement: &mut Statement<'_>,
    params: P,
    sql: &str,
) -> Result<Vec<ResultColumn>, DriverError> {
    statement
        .stream_arrow(params)
        .map(drop)
        .map_err(|error| map_statement_error(error, sql))?;
    (0..statement.column_count())
        .map(|index| {
            let name = statement.column_name(index).map_err(map_error)?.to_owned();
            let carrier = statement.column_type(index);
            let column_type = ColumnType::of(&statement.column_logical_type(index), Some(&carrier));
            ensure_decodable(&name, &carrier, &column_type)?;
            Ok(ResultColumn { name, column_type })
        })
        .collect()
}

/// Fetches at most `limit` rows and then checks for one more row to set `has_more`.
/// The remaining chunks are never fetched.
pub(crate) fn read_batch(
    statement: &Statement<'_>,
    columns: &[ResultColumn],
    limit: usize,
) -> Result<RowBatch, DriverError> {
    let mut rows = statement.raw_query();
    let mut collected = Vec::new();
    let mut has_more = false;
    while let Some(row) = rows.next().map_err(map_error)? {
        if collected.len() == limit {
            has_more = true;
            break;
        }
        let mut values = Vec::with_capacity(columns.len());
        for (index, column) in columns.iter().enumerate() {
            let value = row.get_ref(index).map_err(map_error)?;
            values.push(from_duckdb(value, &column.column_type));
        }
        collected.push(values);
    }
    Ok(RowBatch {
        columns: columns
            .iter()
            .map(|column| ColumnMeta::new(&column.name, Some(column.column_type.name.clone())))
            .collect(),
        rows: collected,
        has_more,
    })
}

pub(crate) fn first_cell(batch: &RowBatch) -> Option<&Value> {
    batch.rows.first().and_then(|row| row.first())
}
