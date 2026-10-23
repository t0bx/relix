/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use relix_driver_api::DriverError;
use tokio_postgres::Row;
use tokio_postgres::types::FromSql;

pub(crate) fn read_column<'a, T: FromSql<'a>>(
    row: &'a Row,
    index: usize,
) -> Result<T, DriverError> {
    row.try_get(index).map_err(map_error)
}
