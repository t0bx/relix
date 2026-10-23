/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::authentication::{http_auth, secured_config, user_name};
use crate::client::TrinoClient;
use crate::session_state::SessionState;
use crate::statement_runner::{QueryOutcome, RunningQuery, run_statement};
use async_trait::async_trait;
use relix_core::{ConnectionConfig, RowBatch};
use relix_driver_api::{DatabaseConnection, DriverError, ServerInfo, SqlConnection};
use relix_driver_http::{EndpointOptions, HttpEndpoint};
use tokio::sync::Mutex;

const PRODUCT_NAME: &str = "Trino";
pub(crate) const DEFAULT_PORT: u16 = 8080;
const PROBE_PATH: &str = "v1/info";

pub(crate) struct TrinoConnection {
    pub(crate) client: TrinoClient,
    pub(crate) initial_session: SessionState,
    pub(crate) session: Mutex<SessionState>,
    pub(crate) running: RunningQuery,
    server_info: ServerInfo,
}

impl TrinoConnection {
    /// Finds the endpoint through `/v1/info`, refuses coordinators that are still
    /// starting and runs a first statement to verify the credentials.
    pub(crate) async fn open(config: &ConnectionConfig) -> Result<Self, DriverError> {
        let config = secured_config(config)?;
        let user = user_name(&config);
        let initial_session = SessionState::from_settings(&config.settings)?;
        let options = EndpointOptions {
            default_port: DEFAULT_PORT,
            probe_path: PROBE_PATH.to_owned(),
            auth: http_auth(&config, &user),
            request_timeout: None,
        };
        let endpoint = HttpEndpoint::connect(&config, options).await?;
        let client = TrinoClient::new(endpoint, user);
        let description = client.server_description().await?;
        if description.starting {
            return Err(DriverError::connection(
                "the Trino coordinator is still starting",
            ));
        }
        let connection = Self {
            client,
            session: Mutex::new(initial_session.clone()),
            initial_session,
            running: RunningQuery::default(),
            server_info: ServerInfo {
                product: PRODUCT_NAME.to_owned(),
                version: description.node_version.version,
            },
        };
        connection.query_rows("SELECT 1").await?;
        Ok(connection)
    }

    /// Runs a metadata or table statement outside the editor session.
    pub(crate) async fn query(&self, sql: &str, limit: usize) -> Result<QueryOutcome, DriverError> {
        let mut session = self.initial_session.clone();
        run_statement(&self.client, sql, &mut session, limit, None).await
    }

    pub(crate) async fn query_rows(&self, sql: &str) -> Result<RowBatch, DriverError> {
        let outcome = self.query(sql, usize::MAX).await?;
        Ok(outcome.batch.unwrap_or_default())
    }
}

#[async_trait]
impl DatabaseConnection for TrinoConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.client.server_description().await.map(drop)
    }

    async fn close(&self) -> Result<(), DriverError> {
        match self.running.current() {
            Some(next_uri) => self.client.abandon(&next_uri).await,
            None => Ok(()),
        }
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}
