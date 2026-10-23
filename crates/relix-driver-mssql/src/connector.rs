/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::encryption::encryption_attempts;
use crate::errors::map_error;
use relix_core::ConnectionConfig;
use relix_driver_api::{DriverError, ErrorKind, with_timeout};
use std::time::Duration;
use tiberius::{AuthMethod, Client, Config};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

pub(crate) type MsSqlClient = Client<Compat<TcpStream>>;

pub const DEFAULT_PORT: u16 = 1433;
const APPLICATION_NAME: &str = "RELIX";

/// Opens TDS sessions to one server. TCP always goes to the configured (possibly
/// tunneled) host and port, while the TDS configuration carries the original host name,
/// which tiberius uses for the TLS server name and certificate validation.
pub(crate) struct Connector {
    host: String,
    port: u16,
    tunneled: bool,
    connect_timeout: Duration,
    tds: Config,
}

impl Connector {
    /// Tries each encryption setting in order and keeps the first one that works, so
    /// later sessions (data connections, reopened sessions) use the same negotiation.
    pub(crate) async fn establish(
        config: &ConnectionConfig,
    ) -> Result<(Self, MsSqlClient), DriverError> {
        let base = base_config(config)?;
        let mut last_error = None;
        for attempt in encryption_attempts(&config.settings.tls)? {
            let mut tds = base.clone();
            attempt.apply(&mut tds);
            let connector = Self {
                host: config.host_or_default().to_owned(),
                port: config.settings.port.unwrap_or(DEFAULT_PORT),
                tunneled: config.tls_server_name.is_some(),
                connect_timeout: config.settings.connect_timeout(),
                tds,
            };
            match connector.open_client().await {
                Ok(client) => return Ok((connector, client)),
                Err(error) if allows_next_attempt(error.kind) => last_error = Some(error),
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| DriverError::internal("no connection attempt was made")))
    }

    pub(crate) async fn open(&self) -> Result<MsSqlClient, DriverError> {
        with_timeout(self.connect_timeout, self.open_client()).await
    }

    async fn open_client(&self) -> Result<MsSqlClient, DriverError> {
        match connect_client(&self.host, self.port, self.tds.clone()).await {
            Err(tiberius::error::Error::Routing { host, port }) => {
                self.follow_redirect(host, port).await
            }
            other => other.map_err(map_error),
        }
    }

    async fn follow_redirect(&self, host: String, port: u16) -> Result<MsSqlClient, DriverError> {
        if self.tunneled {
            return Err(DriverError::connection(format!(
                "the server redirected the connection to {host}:{port}, which is not \
                 reachable through the configured tunnel"
            )));
        }
        let mut tds = self.tds.clone();
        tds.host(&host);
        connect_client(&host, port, tds).await.map_err(map_error)
    }
}

async fn connect_client(
    host: &str,
    port: u16,
    tds: Config,
) -> Result<MsSqlClient, tiberius::error::Error> {
    let tcp = TcpStream::connect((host, port)).await?;
    tcp.set_nodelay(true)?;
    Client::connect(tds, tcp.compat_write()).await
}

fn base_config(config: &ConnectionConfig) -> Result<Config, DriverError> {
    let settings = &config.settings;
    let username = settings
        .username
        .as_deref()
        .map(str::trim)
        .filter(|username| !username.is_empty())
        .ok_or_else(|| {
            DriverError::configuration(
                "a login name is required: RELIX connects with SQL Server authentication",
            )
        })?;
    let password = config
        .password
        .as_ref()
        .map(|secret| secret.expose())
        .unwrap_or_default();
    let mut tds = Config::new();
    tds.host(config.tls_host());
    tds.port(settings.port.unwrap_or(DEFAULT_PORT));
    tds.application_name(APPLICATION_NAME);
    tds.authentication(AuthMethod::sql_server(username, password));
    if let Some(database) = configured_database(config) {
        tds.database(database);
    }
    Ok(tds)
}

pub(crate) fn configured_database(config: &ConnectionConfig) -> Option<&str> {
    config
        .settings
        .database
        .as_deref()
        .map(str::trim)
        .filter(|database| !database.is_empty())
}

fn allows_next_attempt(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::Tls | ErrorKind::Connection | ErrorKind::Protocol | ErrorKind::Io
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use relix_core::{ConnectionSettings, Secret};

    #[test]
    fn a_login_name_is_required() {
        let config = ConnectionConfig::new(ConnectionSettings::default(), None);
        let error = base_config(&config).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Configuration);
    }

    #[test]
    fn tunneled_configs_keep_the_original_host_for_tls() {
        let settings = ConnectionSettings {
            host: Some("sql.internal".into()),
            username: Some("sa".into()),
            database: Some(" ".into()),
            ..ConnectionSettings::default()
        };
        let config =
            ConnectionConfig::new(settings, Some(Secret::new("pw"))).redirected("127.0.0.1", 40000);
        assert_eq!(
            base_config(&config).unwrap().get_addr(),
            "sql.internal:40000"
        );
        assert_eq!(configured_database(&config), None);
    }

    #[test]
    fn only_transport_failures_allow_another_attempt() {
        assert!(allows_next_attempt(ErrorKind::Tls));
        assert!(allows_next_attempt(ErrorKind::Connection));
        assert!(!allows_next_attempt(ErrorKind::Authentication));
    }
}
