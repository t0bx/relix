/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::blocking::run_blocking;
use crate::errors::map_error;
use relix_driver_api::DriverError;
use rusqlite::{Connection, InterruptHandle};
use std::sync::{Arc, Mutex, PoisonError};

pub(crate) struct SharedConnection {
    slot: Arc<Mutex<Option<Connection>>>,
    interrupt: InterruptHandle,
}

impl SharedConnection {
    pub(crate) fn new(connection: Connection) -> Self {
        let interrupt = connection.get_interrupt_handle();
        Self {
            slot: Arc::new(Mutex::new(Some(connection))),
            interrupt,
        }
    }

    /// Runs `operation` on the blocking thread pool with exclusive access to the single
    /// SQLite connection. The editor session, table pages, edits and metadata all go
    /// through here because an in-memory database cannot be shared across connections.
    /// A poisoned lock is recovered, since a panicking operation leaves the connection
    /// itself in a usable state.
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
                .ok_or_else(|| DriverError::connection("the SQLite connection is closed"))?;
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
            match connection {
                Some(connection) => connection.close().map_err(|(_, error)| map_error(error)),
                None => Ok(()),
            }
        })
        .await
    }
}
