/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connector::Connector;
use crate::data_clients::DataClients;
use crate::errors::{connection_closed, map_error};
use crate::managed_client::ManagedClient;
use crate::row_reader::read_column;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect};
use relix_driver_api::{
    DatabaseConnection, DriverError, ParamStyle, ServerInfo, SqlBuilder, SqlConnection,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_postgres::CancelToken;

const PRODUCT_NAME: &str = "PostgreSQL";
const SERVER_IDENTITY_SQL: &str = "SELECT current_setting('server_version'), current_database()";

pub struct PostgresConnection {
    pub(crate) connector: Arc<Connector>,
    pub(crate) default_database: String,
    pub(crate) builder: SqlBuilder,
    server_info: ServerInfo,
    session: Mutex<Option<Arc<ManagedClient>>>,
    cancel_token: CancelToken,
    data_clients: DataClients,
}

impl PostgresConnection {
    pub(crate) async fn open(config: &ConnectionConfig) -> Result<Self, DriverError> {
        let connector = Arc::new(Connector::new(config).await?);
        let session = connector.open(connector.configured_database()).await?;
        let (version, default_database, cancel_token) = {
            let client = session.read().await;
            let row = client
                .query_one(SERVER_IDENTITY_SQL, &[])
                .await
                .map_err(map_error)?;
            let version: String = read_column(&row, 0)?;
            let database: String = read_column(&row, 1)?;
            (version, database, client.cancel_token())
        };
        let data_clients = DataClients::new(connector.clone());
        data_clients.get(&default_database).await?;
        Ok(Self {
            connector,
            default_database,
            builder: SqlBuilder::new(Dialect::Postgres, ParamStyle::Inline),
            server_info: ServerInfo {
                product: PRODUCT_NAME.to_owned(),
                version,
            },
            session: Mutex::new(Some(Arc::new(session))),
            cancel_token,
            data_clients,
        })
    }

    pub(crate) async fn session(&self) -> Result<Arc<ManagedClient>, DriverError> {
        self.session
            .lock()
            .await
            .clone()
            .ok_or_else(connection_closed)
    }

    pub(crate) async fn cancel_session(&self) -> Result<(), DriverError> {
        self.session().await?;
        self.connector.cancel(&self.cancel_token).await
    }

    pub(crate) async fn data_client(
        &self,
        database: Option<&str>,
    ) -> Result<Arc<ManagedClient>, DriverError> {
        self.data_clients.get(self.database_name(database)).await
    }

    pub(crate) fn database_name<'a>(&'a self, database: Option<&'a str>) -> &'a str {
        database
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.default_database)
    }
}

#[async_trait]
impl DatabaseConnection for PostgresConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        if self.session().await?.is_closed() {
            return Err(connection_closed());
        }
        let client = self.data_client(None).await?;
        let guard = client.read().await;
        guard.batch_execute("SELECT 1").await.map_err(map_error)
    }

    async fn close(&self) -> Result<(), DriverError> {
        let session = self.session.lock().await.take();
        if let Some(session) = session {
            ManagedClient::release(session).await;
        }
        self.data_clients.close().await;
        Ok(())
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}
