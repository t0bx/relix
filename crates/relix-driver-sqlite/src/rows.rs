/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::values::from_sqlite;
use relix_core::{ColumnMeta, RowBatch};
use relix_driver_api::DriverError;
use rusqlite::{Params, Statement};

fn column_metadata(statement: &Statement<'_>) -> Vec<ColumnMeta> {
    statement
        .columns()
        .iter()
        .map(|column| ColumnMeta::new(column.name(), column.decl_type().map(str::to_owned)))
        .collect()
}

/// Steps through at most `limit` rows and then checks for one more row to set
/// `has_more`. The remaining rows are never read, so large results are not buffered.
pub(crate) fn read_batch<P: Params>(
    statement: &mut Statement<'_>,
    params: P,
    limit: usize,
) -> Result<RowBatch, DriverError> {
    let columns = column_metadata(statement);
    let width = columns.len();
    let mut rows = statement.query(params).map_err(map_error)?;
    let mut collected = Vec::new();
    let mut has_more = false;
    while let Some(row) = rows.next().map_err(map_error)? {
        if collected.len() == limit {
            has_more = true;
            break;
        }
        let mut values = Vec::with_capacity(width);
        for index in 0..width {
            values.push(from_sqlite(row.get_ref(index).map_err(map_error)?));
        }
        collected.push(values);
    }
    Ok(RowBatch {
        columns,
        rows: collected,
        has_more,
    })
}
