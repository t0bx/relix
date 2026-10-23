/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::blocking::run_blocking;
use crate::connector::OracleConnector;
use crate::errors::connection_closed;
use oracle::Connection;
use relix_driver_api::{DriverError, ErrorKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

const MAX_IDLE_CONNECTIONS: usize = 4;
const METADATA_SETUP: &str = "BEGIN \
     DBMS_METADATA.SET_TRANSFORM_PARAM(DBMS_METADATA.SESSION_TRANSFORM, 'SEGMENT_ATTRIBUTES', FALSE); \
     DBMS_METADATA.SET_TRANSFORM_PARAM(DBMS_METADATA.SESSION_TRANSFORM, 'SQLTERMINATOR', TRUE); \
     END;";

/// Connections for browsing, table pages and edits, kept apart from the editor session so
/// a long statement there does not block them. Operations run on the blocking thread
/// pool; a connection goes back to the idle list only when the operation left it in a
/// known state, otherwise it is closed on that blocking thread.
pub struct DataConnections {
    connector: Arc<OracleConnector>,
    idle: Arc<Mutex<Vec<Connection>>>,
    closed: Arc<AtomicBool>,
}

impl DataConnections {
    pub fn new(connector: Arc<OracleConnector>) -> Self {
        Self {
            connector,
            idle: Arc::new(Mutex::new(Vec::new())),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn run<T, F>(&self, operation: F) -> Result<T, DriverError>
    where
        F: FnOnce(&Connection) -> Result<T, DriverError> + Send + 'static,
        T: Send + 'static,
    {
        if self.closed.load(Ordering::SeqCst) {
            return Err(connection_closed());
        }
        let connector = Arc::clone(&self.connector);
        let idle = Arc::clone(&self.idle);
        let closed = Arc::clone(&self.closed);
        run_blocking(move || {
            let reused = idle.lock().unwrap_or_else(PoisonError::into_inner).pop();
            let connection = match reused {
                Some(connection) => connection,
                None => connector.open(&[METADATA_SETUP])?,
            };
            let result = operation(&connection);
            if keeps_connection_usable(&result) && !closed.load(Ordering::SeqCst) {
                let mut idle = idle.lock().unwrap_or_else(PoisonError::into_inner);
                if idle.len() < MAX_IDLE_CONNECTIONS {
                    idle.push(connection);
                }
            }
            result
        })
        .await
    }

    pub async fn close(&self) -> Result<(), DriverError> {
        self.closed.store(true, Ordering::SeqCst);
        let idle = Arc::clone(&self.idle);
        run_blocking(move || {
            let connections =
                std::mem::take(&mut *idle.lock().unwrap_or_else(PoisonError::into_inner));
            for connection in connections {
                let _ = connection.close();
            }
            Ok(())
        })
        .await
    }
}

fn keeps_connection_usable<T>(result: &Result<T, DriverError>) -> bool {
    match result {
        Ok(_) => true,
        Err(error) => matches!(
            error.kind,
            ErrorKind::Query
                | ErrorKind::ConcurrentModification
                | ErrorKind::AmbiguousRow
                | ErrorKind::Unsupported
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_statement_level_failures_keep_a_connection() {
        assert!(keeps_connection_usable::<()>(&Ok(())));
        assert!(keeps_connection_usable::<()>(&Err(DriverError::query("x"))));
        assert!(!keeps_connection_usable::<()>(&Err(
            DriverError::connection("x")
        )));
        assert!(!keeps_connection_usable::<()>(&Err(DriverError::new(
            ErrorKind::Cancelled,
            "x"
        ))));
    }
}
