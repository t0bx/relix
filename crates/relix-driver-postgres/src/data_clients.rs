/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connector::Connector;
use crate::errors::{connection_closed, map_error};
use crate::managed_client::ManagedClient;
use relix_driver_api::DriverError;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

const DATA_SESSION_SETUP: &str = "SET standard_conforming_strings = on";

pub(crate) struct DataClients {
    connector: Arc<Connector>,
    state: Mutex<DataClientsState>,
}

#[derive(Default)]
struct DataClientsState {
    clients: HashMap<String, Arc<ManagedClient>>,
    closed: bool,
}

impl DataClients {
    pub(crate) fn new(connector: Arc<Connector>) -> Self {
        Self {
            connector,
            state: Mutex::new(DataClientsState::default()),
        }
    }

    pub(crate) async fn get(&self, database: &str) -> Result<Arc<ManagedClient>, DriverError> {
        let mut state = self.state.lock().await;
        if state.closed {
            return Err(connection_closed());
        }
        if let Some(client) = state
            .clients
            .get(database)
            .filter(|client| !client.is_closed())
        {
            return Ok(client.clone());
        }
        let client = Arc::new(self.open(database).await?);
        if let Some(stale) = state.clients.insert(database.to_owned(), client.clone()) {
            ManagedClient::release(stale).await;
        }
        Ok(client)
    }

    pub(crate) async fn close(&self) {
        let clients = {
            let mut state = self.state.lock().await;
            state.closed = true;
            std::mem::take(&mut state.clients)
        };
        for client in clients.into_values() {
            ManagedClient::release(client).await;
        }
    }

    async fn open(&self, database: &str) -> Result<ManagedClient, DriverError> {
        let managed = self.connector.open(database).await?;
        managed
            .read()
            .await
            .batch_execute(DATA_SESSION_SETUP)
            .await
            .map_err(map_error)?;
        Ok(managed)
    }
}
