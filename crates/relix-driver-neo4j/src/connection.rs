/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::connect::{LiveGraph, configured_database, open_graph};
use crate::cypher::fetch_all;
use crate::errors::connection_closed;
use crate::server_identity::{current_database, server_info};
use async_trait::async_trait;
use neo4rs::{Txn, query};
use relix_core::ConnectionConfig;
use relix_driver_api::{
    CancelSignal, DatabaseConnection, DriverError, ServerInfo, SqlConnection, with_timeout,
};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

const FALLBACK_DATABASE: &str = "neo4j";

pub struct Neo4jConnection {
    live: RwLock<Option<Arc<LiveGraph>>>,
    pub(crate) transaction: Mutex<Option<Txn>>,
    server_info: ServerInfo,
    pub(crate) default_database: String,
    pub(crate) session_token: String,
    pub(crate) cancel_signal: CancelSignal,
}

impl Neo4jConnection {
    pub(crate) async fn open(config: &ConnectionConfig) -> Result<Self, DriverError> {
        with_timeout(config.settings.connect_timeout(), async {
            let live = open_graph(config).await?;
            let server_info = server_info(&live.graph).await?;
            let default_database = match configured_database(config) {
                Some(database) => database.to_owned(),
                None => current_database(&live.graph)
                    .await?
                    .unwrap_or_else(|| FALLBACK_DATABASE.to_owned()),
            };
            Ok(Self {
                live: RwLock::new(Some(Arc::new(live))),
                transaction: Mutex::new(None),
                server_info,
                default_database,
                session_token: uuid::Uuid::new_v4().to_string(),
                cancel_signal: CancelSignal::new(),
            })
        })
        .await
    }

    pub(crate) async fn live(&self) -> Result<Arc<LiveGraph>, DriverError> {
        self.live.read().await.clone().ok_or_else(connection_closed)
    }

    pub(crate) fn database_name<'a>(&'a self, database: Option<&'a str>) -> &'a str {
        database
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.default_database)
    }
}

#[async_trait]
impl DatabaseConnection for Neo4jConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let live = self.live().await?;
        fetch_all(&live.graph, None, query("RETURN 1 AS ok")).await?;
        Ok(())
    }

    async fn close(&self) -> Result<(), DriverError> {
        self.cancel_signal.cancel();
        if let Some(transaction) = self.transaction.lock().await.take() {
            let _ = transaction.rollback().await;
        }
        self.live.write().await.take();
        Ok(())
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}
