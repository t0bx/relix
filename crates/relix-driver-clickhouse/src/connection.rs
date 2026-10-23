/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::{ClickHouseClient, QueryRequest};
use crate::credentials::Credentials;
use crate::metadata_rows::text_at;
use crate::session::EditorSession;
use async_trait::async_trait;
use relix_core::ConnectionConfig;
use relix_driver_api::{DatabaseConnection, DriverError, ServerInfo, SqlConnection};
use relix_driver_http::{EndpointOptions, HttpAuth, HttpEndpoint};

const PRODUCT_NAME: &str = "ClickHouse";
pub(crate) const DEFAULT_HTTP_PORT: u16 = 8123;
const PROBE_PATH: &str = "ping";
const IDENTITY_SQL: &str = "SELECT version(), currentDatabase()";

pub(crate) struct ClickHouseConnection {
    pub(crate) client: ClickHouseClient,
    pub(crate) session: EditorSession,
    pub(crate) default_database: String,
    server_info: ServerInfo,
}

impl ClickHouseConnection {
    /// Finds the HTTP(S) endpoint with an unauthenticated `/ping`, then verifies the
    /// credentials and the configured database with a first query.
    pub(crate) async fn open(config: &ConnectionConfig) -> Result<Self, DriverError> {
        let credentials = Credentials::from_config(config)?;
        let session = EditorSession::new(&config.settings)?;
        let options = EndpointOptions {
            default_port: DEFAULT_HTTP_PORT,
            probe_path: PROBE_PATH.to_owned(),
            auth: HttpAuth::None,
            request_timeout: None,
        };
        let endpoint = HttpEndpoint::connect(config, options).await?;
        let database = config
            .settings
            .database
            .as_deref()
            .map(str::trim)
            .filter(|database| !database.is_empty())
            .map(str::to_owned);
        let client = ClickHouseClient::new(endpoint, credentials, database);
        let identity = client.rows(&QueryRequest::new(IDENTITY_SQL)).await?;
        let row = identity
            .rows
            .first()
            .ok_or_else(|| DriverError::internal("the server did not report its version"))?;
        Ok(Self {
            server_info: ServerInfo {
                product: PRODUCT_NAME.to_owned(),
                version: text_at(row, 0),
            },
            default_database: text_at(row, 1),
            client,
            session,
        })
    }
}

#[async_trait]
impl DatabaseConnection for ClickHouseConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.client
            .rows(&QueryRequest::new("SELECT 1"))
            .await
            .map(drop)
    }

    async fn close(&self) -> Result<(), DriverError> {
        self.session.cancel(&self.client).await
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}
