/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DatabaseConnection, DriverError, SqlConnection};
use std::sync::{Arc, PoisonError, RwLock};

#[derive(Default)]
pub(crate) struct Session {
    connection: RwLock<Option<Arc<dyn DatabaseConnection>>>,
}

impl Session {
    pub(crate) fn open(&self, connection: Box<dyn DatabaseConnection>) -> Result<(), DriverError> {
        let mut slot = self
            .connection
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.is_some() {
            return Err(DriverError::configuration(
                "this connector process is already connected",
            ));
        }
        *slot = Some(Arc::from(connection));
        Ok(())
    }

    pub(crate) fn current(&self) -> Result<Arc<dyn DatabaseConnection>, DriverError> {
        self.connection
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| DriverError::connection("connect has not been called yet"))
    }

    pub(crate) fn take(&self) -> Option<Arc<dyn DatabaseConnection>> {
        self.connection
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

pub(crate) fn sql(connection: &dyn DatabaseConnection) -> Result<&dyn SqlConnection, DriverError> {
    connection.as_sql().ok_or_else(|| {
        DriverError::unsupported("protocol version 1 only supports relational connectors")
    })
}
