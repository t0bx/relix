/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::driver::DEFAULT_PORT;
use crate::query_language::QueryLanguage;
use async_trait::async_trait;
use relix_core::ConnectionConfig;
use relix_driver_api::{
    CancelSignal, DatabaseConnection, DriverError, ServerInfo, SqlConnection, with_timeout,
};
use relix_driver_http::{EndpointOptions, HttpAuth, HttpEndpoint, Method};
use serde::Deserialize;
use tokio::sync::RwLock;

const PING_PATH: &str = "/ping";
pub(crate) const DATABASES_PATH: &str = "/api/v3/configure/database?format=json";
const DATABASE_PROPERTY: &str = "database";

#[derive(Debug, Deserialize)]
struct Ping {
    product_name: Option<String>,
    version: Option<String>,
}

pub struct InfluxDbConnection {
    endpoint: RwLock<Option<HttpEndpoint>>,
    server_info: ServerInfo,
    pub(crate) database: Option<String>,
    pub(crate) language: QueryLanguage,
    pub(crate) cancel_signal: CancelSignal,
}

impl InfluxDbConnection {
    /// Connects with the token from the password field (`Authorization: Bearer`), or
    /// without credentials for servers started with `--without-auth`. The database list
    /// is requested right away because it requires a valid token even where `/ping` is
    /// public, so a wrong token fails the connection instead of the first query.
    pub(crate) async fn open(config: &ConnectionConfig) -> Result<Self, DriverError> {
        let language = QueryLanguage::from_settings(&config.settings)?;
        with_timeout(config.settings.connect_timeout(), async {
            let options = EndpointOptions {
                default_port: DEFAULT_PORT,
                probe_path: PING_PATH.to_owned(),
                auth: token_auth(config),
                request_timeout: config.settings.query_timeout(),
            };
            let endpoint = HttpEndpoint::connect(config, options).await?;
            let ping: Ping = endpoint
                .json(endpoint.request(Method::GET, PING_PATH))
                .await?;
            endpoint
                .text(endpoint.request(Method::GET, DATABASES_PATH))
                .await?;
            Ok(Self {
                endpoint: RwLock::new(Some(endpoint)),
                server_info: ServerInfo {
                    product: ping.product_name.unwrap_or_else(|| "InfluxDB 3".to_owned()),
                    version: ping.version.unwrap_or_else(|| "unknown".to_owned()),
                },
                database: configured_database(config),
                language,
                cancel_signal: CancelSignal::new(),
            })
        })
        .await
    }

    pub(crate) async fn endpoint(&self) -> Result<HttpEndpoint, DriverError> {
        self.endpoint
            .read()
            .await
            .clone()
            .ok_or_else(|| DriverError::connection("connection is closed"))
    }

    pub(crate) fn database_or<'a>(
        &'a self,
        database: Option<&'a str>,
    ) -> Result<&'a str, DriverError> {
        database
            .filter(|name| !name.is_empty())
            .or(self.database.as_deref())
            .ok_or_else(|| {
                DriverError::configuration(
                    "no database selected: set the data source's database (or the 'database' property)",
                )
            })
    }
}

fn token_auth(config: &ConnectionConfig) -> HttpAuth {
    match &config.password {
        Some(token) if !token.expose().trim().is_empty() => HttpAuth::Bearer(token.clone()),
        _ => HttpAuth::None,
    }
}

fn configured_database(config: &ConnectionConfig) -> Option<String> {
    config
        .settings
        .database
        .as_deref()
        .map(str::trim)
        .filter(|database| !database.is_empty())
        .or_else(|| config.settings.property(DATABASE_PROPERTY))
        .map(str::to_owned)
}

#[async_trait]
impl DatabaseConnection for InfluxDbConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let endpoint = self.endpoint().await?;
        endpoint
            .send(endpoint.request(Method::GET, PING_PATH))
            .await?;
        Ok(())
    }

    async fn close(&self) -> Result<(), DriverError> {
        self.cancel_signal.cancel();
        self.endpoint.write().await.take();
        Ok(())
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::{ConnectionSettings, Secret};

    #[test]
    fn database_comes_from_the_field_or_the_property() {
        let mut settings = ConnectionSettings::default();
        settings
            .properties
            .insert("database".into(), "metrics".into());
        let config = ConnectionConfig::new(settings.clone(), None);
        assert_eq!(configured_database(&config).as_deref(), Some("metrics"));
        settings.database = Some("sensors".into());
        let config = ConnectionConfig::new(settings, Some(Secret::new("  ")));
        assert_eq!(configured_database(&config).as_deref(), Some("sensors"));
        assert!(matches!(token_auth(&config), HttpAuth::None));
    }
}
