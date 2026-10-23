/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use oracle::InitParams;
use relix_driver_api::DriverError;

/// Loads the Oracle Client library (once per process; a failed attempt is retried by the
/// next connection). `client_lib_dir` only takes effect for the first successful load,
/// because ODPI-C keeps one global context. A missing Instant Client becomes a
/// configuration error explaining how to install it instead of failing later.
pub fn load_client_library(client_lib_dir: Option<&str>) -> Result<(), DriverError> {
    let mut params = InitParams::new();
    if let Some(directory) = client_lib_dir {
        params.oracle_client_lib_dir(directory).map_err(map_error)?;
    }
    params.default_driver_name("RELIX").map_err(map_error)?;
    params.init().map(drop).map_err(map_error)
}
