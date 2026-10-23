/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog_rows::run_batch;
use crate::connector::{Connector, MsSqlClient};
use crate::errors::connection_closed;
use relix_driver_api::{DriverError, ErrorKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

const MAX_IDLE_CLIENTS: usize = 4;
const DATA_SESSION_SETUP: &str = "SET DATEFORMAT ymd";

/// Connections for browsing, table pages, edits and cancellation, kept apart from the
/// editor session so a long statement there does not block them. `DATEFORMAT ymd` makes
/// the text form of `datetime` values convert back unambiguously in edits.
pub struct DataClients {
    connector: Arc<Connector>,
    idle: Mutex<Vec<MsSqlClient>>,
    closed: AtomicBool,
}

/// A borrowed data connection. It returns to the pool only after [`DataLease::finish`]
/// saw a result that leaves the connection in a known state; a lease dropped mid-way
/// (for example by a timeout) closes its connection.
pub struct DataLease<'a> {
    owner: &'a DataClients,
    client: Option<MsSqlClient>,
}

impl DataClients {
    pub fn new(connector: Arc<Connector>) -> Self {
        Self {
            connector,
            idle: Mutex::new(Vec::new()),
            closed: AtomicBool::new(false),
        }
    }

    pub async fn lease(&self) -> Result<DataLease<'_>, DriverError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(connection_closed());
        }
        let client = match self.take_idle() {
            Some(client) => client,
            None => self.open().await?,
        };
        Ok(DataLease {
            owner: self,
            client: Some(client),
        })
    }

    pub async fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let clients =
            std::mem::take(&mut *self.idle.lock().unwrap_or_else(PoisonError::into_inner));
        for client in clients {
            let _ = client.close().await;
        }
    }

    fn take_idle(&self) -> Option<MsSqlClient> {
        self.idle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop()
    }

    async fn open(&self) -> Result<MsSqlClient, DriverError> {
        let mut client = self.connector.open().await?;
        run_batch(&mut client, DATA_SESSION_SETUP).await?;
        Ok(client)
    }

    fn give_back(&self, client: MsSqlClient) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let mut idle = self.idle.lock().unwrap_or_else(PoisonError::into_inner);
        if idle.len() < MAX_IDLE_CLIENTS {
            idle.push(client);
        }
    }
}

impl DataLease<'_> {
    pub fn client(&mut self) -> Result<&mut MsSqlClient, DriverError> {
        self.client.as_mut().ok_or_else(connection_closed)
    }

    pub fn finish<T>(mut self, result: Result<T, DriverError>) -> Result<T, DriverError> {
        if keeps_connection_usable(&result)
            && let Some(client) = self.client.take()
        {
            self.owner.give_back(client);
        }
        result
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
                | ErrorKind::Configuration
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
            ErrorKind::Timeout,
            "x"
        ))));
    }
}
