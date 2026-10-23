/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::row_reader::read_column;
use relix_driver_api::DriverError;
use tokio_postgres::Client;

/// Runs CockroachDB's `SHOW CREATE`, which works for tables, views and sequences and
/// keeps engine-specific clauses (hidden columns, column families, hash sharding).
pub(crate) async fn show_create(
    client: &Client,
    qualified_name: &str,
) -> Result<Option<String>, DriverError> {
    let row = client
        .query_opt(&format!("SHOW CREATE {qualified_name}"), &[])
        .await
        .map_err(map_error)?;
    row.map(|row| read_column::<String>(&row, 1).map(|ddl| format!("{};", ddl.trim_end())))
        .transpose()
}
