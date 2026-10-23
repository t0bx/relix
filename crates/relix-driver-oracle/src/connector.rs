/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client_library::load_client_library;
use crate::connect_target::ConnectTarget;
use crate::errors::map_error;
use oracle::{Connection, Connector};
use relix_driver_api::DriverError;

/// Text forms used for temporal values are also what the session parses implicitly, so
/// edited dates and timestamps convert back without explicit format models.
const SESSION_SETUP: &str = "ALTER SESSION SET \
     NLS_DATE_FORMAT = 'YYYY-MM-DD HH24:MI:SS' \
     NLS_TIMESTAMP_FORMAT = 'YYYY-MM-DD HH24:MI:SS.FF' \
     NLS_TIMESTAMP_TZ_FORMAT = 'YYYY-MM-DD HH24:MI:SS.FF TZH:TZM' \
     NLS_NUMERIC_CHARACTERS = '.,'";

pub struct OracleConnector {
    target: ConnectTarget,
}

impl OracleConnector {
    pub fn new(target: ConnectTarget) -> Self {
        Self { target }
    }

    pub fn service_name(&self) -> &str {
        &self.target.service_name
    }

    /// Opens a session and runs the shared setup plus `extra_setup`. Blocking: call it
    /// from the blocking thread pool.
    pub fn open(&self, extra_setup: &[&str]) -> Result<Connection, DriverError> {
        load_client_library(self.target.client_lib_dir.as_deref())?;
        let connection = Connector::new(
            self.target.username.as_str(),
            self.target.password.as_str(),
            self.target.connect_string.as_str(),
        )
        .driver_name("RELIX")
        .connect()
        .map_err(map_error)?;
        for statement in std::iter::once(&SESSION_SETUP).chain(extra_setup) {
            connection.execute(statement, &[]).map_err(map_error)?;
        }
        Ok(connection)
    }
}
