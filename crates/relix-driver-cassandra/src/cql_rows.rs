/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::{map_execution_error, protocol_error};
use relix_driver_api::DriverError;
use scylla::client::session::Session;
use scylla::deserialize::row::DeserializeRow;
use scylla::serialize::row::SerializeRow;

pub(crate) async fn select_rows<R>(
    session: &Session,
    cql: &str,
    values: impl SerializeRow,
) -> Result<Vec<R>, DriverError>
where
    R: for<'frame, 'metadata> DeserializeRow<'frame, 'metadata>,
{
    let result = session
        .query_unpaged(cql, values)
        .await
        .map_err(|error| map_execution_error(&error))?;
    let rows = result.into_rows_result().map_err(protocol_error)?;
    rows.rows::<R>()
        .map_err(protocol_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(protocol_error)
}
