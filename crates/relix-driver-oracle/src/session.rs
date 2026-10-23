/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::blocking::run_blocking;
use crate::errors::{connection_closed, map_error};
use oracle::Connection;
use relix_driver_api::DriverError;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;

/// The long-lived editor session. Statements run one at a time on the blocking thread
/// pool; `cancel` interrupts the running call from another thread with an OCI break,
/// which leaves the session (and its open transaction) usable.
pub struct Session {
    connection: Arc<Connection>,
    exclusive: Mutex<()>,
    running: Arc<AtomicBool>,
    closed: AtomicBool,
}

impl Session {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection: Arc::new(connection),
            exclusive: Mutex::new(()),
            running: Arc::new(AtomicBool::new(false)),
            closed: AtomicBool::new(false),
        }
    }

    pub async fn run<T, F>(&self, operation: F) -> Result<T, DriverError>
    where
        F: FnOnce(&Connection) -> Result<T, DriverError> + Send + 'static,
        T: Send + 'static,
    {
        let _exclusive = self.exclusive.lock().await;
        if self.closed.load(Ordering::SeqCst) {
            return Err(connection_closed());
        }
        let connection = Arc::clone(&self.connection);
        let running = Arc::clone(&self.running);
        run_blocking(move || {
            running.store(true, Ordering::SeqCst);
            let result = operation(&connection);
            running.store(false, Ordering::SeqCst);
            result
        })
        .await
    }

    pub async fn cancel(&self) -> Result<(), DriverError> {
        if !self.running.load(Ordering::SeqCst) {
            return Ok(());
        }
        let connection = Arc::clone(&self.connection);
        run_blocking(move || connection.break_execution().map_err(map_error)).await
    }

    pub async fn close(&self) -> Result<(), DriverError> {
        let _exclusive = match self.exclusive.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                let _ = self.cancel().await;
                self.exclusive.lock().await
            }
        };
        if self.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let connection = Arc::clone(&self.connection);
        run_blocking(move || connection.close().map_err(map_error)).await
    }
}
