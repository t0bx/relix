/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::values::from_sql_value;
use oracle::{ResultSet, Row};
use relix_core::{ColumnMeta, RowBatch};

/// Collects at most `limit` rows. One more row is fetched to tell whether the result was
/// truncated; the cursor is then closed instead of reading the remainder.
pub fn read_rows(rows: ResultSet<'_, Row>, limit: usize) -> Result<RowBatch, oracle::Error> {
    let columns = rows
        .column_info()
        .iter()
        .map(|column| ColumnMeta::new(column.name(), Some(column.oracle_type().to_string())))
        .collect();
    let mut batch = RowBatch {
        columns,
        rows: Vec::new(),
        has_more: false,
    };
    for row in rows {
        let row = row?;
        if batch.rows.len() == limit {
            batch.has_more = true;
            break;
        }
        batch
            .rows
            .push(row.sql_values().iter().map(from_sql_value).collect());
    }
    Ok(batch)
}
