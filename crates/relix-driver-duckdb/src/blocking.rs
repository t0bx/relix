/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::DriverError;

pub(crate) async fn run_blocking<T, F>(operation: F) -> Result<T, DriverError>
where
    F: FnOnce() -> Result<T, DriverError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| DriverError::internal(format!("DuckDB worker task failed: {error}")))?
}
