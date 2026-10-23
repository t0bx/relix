/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use oracle::sql_type::ToSql;
use oracle::{Connection, Row};
use relix_driver_api::DriverError;

pub fn query_rows(
    connection: &Connection,
    sql: &str,
    params: &[&dyn ToSql],
) -> Result<Vec<Row>, DriverError> {
    connection
        .query(sql, params)
        .map_err(map_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_error)
}

pub fn text_at(row: &Row, index: usize) -> Option<String> {
    row.get::<usize, Option<String>>(index).ok().flatten()
}

pub fn int_at(row: &Row, index: usize) -> Option<i64> {
    row.get::<usize, Option<i64>>(index).ok().flatten()
}
