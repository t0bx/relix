/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use relix_driver_api::DriverError;
use rusqlite::{Connection, Params, Row};

pub(crate) fn query_all<T, P, F>(
    connection: &Connection,
    sql: &str,
    params: P,
    map: F,
) -> Result<Vec<T>, DriverError>
where
    P: Params,
    F: FnMut(&Row<'_>) -> rusqlite::Result<T>,
{
    let mut statement = connection.prepare(sql).map_err(map_error)?;
    let rows = statement.query_map(params, map).map_err(map_error)?;
    rows.collect::<rusqlite::Result<Vec<T>>>()
        .map_err(map_error)
}
