/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connect::{LiveSession, configured_keyspace, open_session};
use crate::errors::{connection_closed, map_execution_error};
use crate::product::detect_server;
use async_trait::async_trait;
use relix_core::ConnectionConfig;
use relix_driver_api::{CancelSignal, DatabaseConnection, DriverError, ServerInfo, SqlConnection};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

const PING_CQL: &str = "SELECT now() FROM system.local";

pub struct CassandraConnection {
    live: RwLock<Option<Arc<LiveSession>>>,
    server_info: ServerInfo,
    pub(crate) default_keyspace: Option<String>,
    pub(crate) query_timeout: Option<Duration>,
    pub(crate) cancel_signal: CancelSignal,
}

impl CassandraConnection {
    pub(crate) async fn open(config: &ConnectionConfig) -> Result<Self, DriverError> {
        let live = open_session(config).await?;
        let server_info = detect_server(&live.session).await?;
        Ok(Self {
            live: RwLock::new(Some(Arc::new(live))),
            server_info,
            default_keyspace: configured_keyspace(config).map(str::to_owned),
            query_timeout: config.settings.query_timeout(),
            cancel_signal: CancelSignal::default(),
        })
    }

    pub(crate) async fn live(&self) -> Result<Arc<LiveSession>, DriverError> {
        self.live.read().await.clone().ok_or_else(connection_closed)
    }
}

#[async_trait]
impl DatabaseConnection for CassandraConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let live = self.live().await?;
        live.session
            .query_unpaged(PING_CQL, ())
            .await
            .map(|_| ())
            .map_err(|error| map_execution_error(&error))
    }

    async fn close(&self) -> Result<(), DriverError> {
        self.cancel_signal.cancel();
        self.live.write().await.take();
        Ok(())
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}
