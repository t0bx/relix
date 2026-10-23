/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::blocking::run_blocking;
use duckdb::{Connection, InterruptHandle};
use relix_driver_api::DriverError;
use std::sync::{Arc, Mutex, PoisonError};

pub(crate) struct SharedConnection {
    slot: Arc<Mutex<Option<Connection>>>,
    interrupt: Arc<InterruptHandle>,
}

impl SharedConnection {
    pub(crate) fn new(connection: Connection) -> Self {
        let interrupt = connection.interrupt_handle();
        Self {
            slot: Arc::new(Mutex::new(Some(connection))),
            interrupt,
        }
    }

    /// Runs `operation` on the blocking thread pool with exclusive access to the
    /// connection. A poisoned lock is recovered, since a panicking operation leaves the
    /// DuckDB connection itself in a usable state.
    pub(crate) async fn run<T, F>(&self, operation: F) -> Result<T, DriverError>
    where
        F: FnOnce(&mut Connection) -> Result<T, DriverError> + Send + 'static,
        T: Send + 'static,
    {
        let slot = Arc::clone(&self.slot);
        run_blocking(move || {
            let mut guard = slot.lock().unwrap_or_else(PoisonError::into_inner);
            let connection = guard
                .as_mut()
                .ok_or_else(|| DriverError::connection("the DuckDB connection is closed"))?;
            operation(connection)
        })
        .await
    }

    pub(crate) fn interrupt(&self) {
        self.interrupt.interrupt();
    }

    pub(crate) async fn close(&self) -> Result<(), DriverError> {
        self.interrupt();
        let slot = Arc::clone(&self.slot);
        run_blocking(move || {
            let connection = slot.lock().unwrap_or_else(PoisonError::into_inner).take();
            drop(connection);
            Ok(())
        })
        .await
    }
}
