/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connector::{Connector, MsSqlClient};
use crate::errors::{
    connection_closed, connection_lost, map_error, map_statement_error, statement_cancelled,
};
use crate::server_details::process_id;
use relix_driver_api::DriverError;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use tokio::sync::{Mutex, MutexGuard};

pub enum SessionSlot {
    Active(Box<MsSqlClient>),
    Killed,
    Lost,
    Closed,
}

/// The long-lived editor session. Cancelling ends it with `KILL` from another
/// connection, after which the next statement transparently opens a new session; a
/// session lost for any other reason stays lost so the data source is reported as
/// disconnected.
pub struct Session {
    connector: Arc<Connector>,
    slot: Mutex<SessionSlot>,
    process_id: AtomicI32,
    running: AtomicBool,
    kill_requested: AtomicBool,
}

pub struct RunningStatement<'a> {
    running: &'a AtomicBool,
}

impl Drop for RunningStatement<'_> {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

impl Session {
    pub fn new(connector: Arc<Connector>, client: MsSqlClient, process_id: i32) -> Self {
        Self {
            connector,
            slot: Mutex::new(SessionSlot::Active(Box::new(client))),
            process_id: AtomicI32::new(process_id),
            running: AtomicBool::new(false),
            kill_requested: AtomicBool::new(false),
        }
    }

    pub async fn lock(&self) -> MutexGuard<'_, SessionSlot> {
        self.slot.lock().await
    }

    pub fn try_lock(&self) -> Option<MutexGuard<'_, SessionSlot>> {
        self.slot.try_lock().ok()
    }

    pub async fn client<'a>(
        &self,
        slot: &'a mut SessionSlot,
    ) -> Result<&'a mut MsSqlClient, DriverError> {
        if self.kill_requested.swap(false, Ordering::SeqCst)
            && matches!(slot, SessionSlot::Active(_))
        {
            *slot = SessionSlot::Killed;
        }
        if matches!(slot, SessionSlot::Killed) {
            let mut client = self.connector.open().await?;
            self.process_id
                .store(process_id(&mut client).await?, Ordering::SeqCst);
            *slot = SessionSlot::Active(Box::new(client));
        }
        match slot {
            SessionSlot::Active(client) => Ok(client),
            SessionSlot::Lost => Err(connection_lost()),
            SessionSlot::Killed | SessionSlot::Closed => Err(connection_closed()),
        }
    }

    pub fn start_statement(&self) -> RunningStatement<'_> {
        self.running.store(true, Ordering::SeqCst);
        RunningStatement {
            running: &self.running,
        }
    }

    pub fn statement_failed(
        &self,
        slot: &mut SessionSlot,
        error: tiberius::error::Error,
        statement: &str,
    ) -> DriverError {
        if self.kill_requested.swap(false, Ordering::SeqCst) {
            *slot = SessionSlot::Killed;
            return statement_cancelled();
        }
        let mapped = map_statement_error(error, statement);
        if mapped.kind.is_connection_loss() {
            *slot = SessionSlot::Lost;
        }
        mapped
    }

    /// Marks the running statement for cancellation and returns the server process id
    /// to kill, or `None` when nothing is running.
    pub fn request_kill(&self) -> Option<i32> {
        if !self.running.load(Ordering::SeqCst) {
            return None;
        }
        self.kill_requested.store(true, Ordering::SeqCst);
        Some(self.process_id.load(Ordering::SeqCst))
    }

    pub fn kill_failed(&self) {
        self.kill_requested.store(false, Ordering::SeqCst);
    }

    pub async fn close(&self, mut slot: MutexGuard<'_, SessionSlot>) -> Result<(), DriverError> {
        let previous = std::mem::replace(&mut *slot, SessionSlot::Closed);
        drop(slot);
        match previous {
            SessionSlot::Active(client) => client.close().await.map_err(map_error),
            _ => Ok(()),
        }
    }
}
