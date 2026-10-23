/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::client::SqlApiClient;
use crate::api::models::{ResultSet, StatementRequest};
use crate::auth::Credentials;
use crate::result::{RawRows, collect_rows};
use crate::session_context::StatementContext;
use crate::settings::SnowflakeSettings;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect};
use relix_driver_api::{
    DatabaseConnection, DriverError, ParamStyle, ServerInfo, SqlBuilder, SqlConnection,
};
use relix_driver_http::{HttpAuth, HttpEndpoint, RunningJob};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

const PRODUCT_NAME: &str = "Snowflake";
const VERSION_SQL: &str = "SELECT CURRENT_VERSION()";

/// A Snowflake data source reached through the SQL API v2. Every statement is an
/// independent HTTPS request carrying the current database, schema, warehouse and role.
pub struct SnowflakeConnection {
    client: SqlApiClient,
    context: Mutex<StatementContext>,
    statement_timeout_secs: Option<u64>,
    pub(crate) running: RunningJob<String>,
    pub(crate) builder: SqlBuilder,
    server_info: ServerInfo,
    closed: AtomicBool,
}

impl SnowflakeConnection {
    pub(crate) async fn open(config: &ConnectionConfig) -> Result<Self, DriverError> {
        let settings = SnowflakeSettings::from_config(config)?;
        let credentials = Credentials::load(&settings, config.password.as_ref()).await?;
        let endpoint =
            HttpEndpoint::at_base_url(config, settings.base_url.clone(), HttpAuth::None, None)
                .await?;
        let mut connection = Self {
            client: SqlApiClient::new(endpoint, credentials),
            context: Mutex::new(settings.context),
            statement_timeout_secs: settings.statement_timeout_secs,
            running: RunningJob::default(),
            builder: SqlBuilder::new(Dialect::Snowflake, ParamStyle::Inline),
            server_info: ServerInfo::default(),
            closed: AtomicBool::new(false),
        };
        connection.server_info = ServerInfo {
            product: PRODUCT_NAME.to_owned(),
            version: connection.scalar(VERSION_SQL).await?.unwrap_or_default(),
        };
        Ok(connection)
    }

    pub(crate) fn client(&self) -> &SqlApiClient {
        &self.client
    }

    pub(crate) fn context(&self) -> StatementContext {
        self.lock_context().clone()
    }

    pub(crate) fn replace_context(&self, context: StatementContext) {
        *self.lock_context() = context;
    }

    /// Submits one statement with the current context and waits for its first partition.
    pub(crate) async fn submit(
        &self,
        statement: &str,
        job: Option<&RunningJob<String>>,
    ) -> Result<ResultSet, DriverError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(DriverError::connection("the connection is closed"));
        }
        let context = self.context();
        let request = StatementRequest {
            statement,
            timeout: self.statement_timeout_secs,
            context: &context,
        };
        self.client.execute(&request, job).await
    }

    pub(crate) async fn query_rows(&self, sql: &str, limit: usize) -> Result<RawRows, DriverError> {
        let first = self.submit(sql, None).await?;
        collect_rows(&self.client, first, limit).await
    }

    pub(crate) async fn scalar(&self, sql: &str) -> Result<Option<String>, DriverError> {
        let rows = self.query_rows(sql, 1).await?;
        Ok(rows
            .rows
            .into_iter()
            .next()
            .and_then(|row| row.into_iter().next().flatten()))
    }

    fn lock_context(&self) -> MutexGuard<'_, StatementContext> {
        self.context
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[async_trait]
impl DatabaseConnection for SnowflakeConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.scalar(VERSION_SQL).await.map(|_| ())
    }

    async fn close(&self) -> Result<(), DriverError> {
        self.closed.store(true, Ordering::Release);
        if let Some(handle) = self.running.request_cancel() {
            let _ = self.client.cancel(&handle).await;
        }
        Ok(())
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}
