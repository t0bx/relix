/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::{is_tls_failure, map_error};
use crate::flavor::Flavor;
use crate::managed_client::ManagedClient;
use crate::server_name_tls::ServerNameTls;
use crate::tls::tls_connector;
use relix_core::{ConnectionConfig, TlsMode};
use relix_driver_api::{DriverError, with_timeout};
use std::time::Duration;
use tokio_postgres::config::SslMode;
use tokio_postgres::tls::{MakeTlsConnect, TlsConnect};
use tokio_postgres::{CancelToken, Config, NoTls, Socket};

const APPLICATION_NAME: &str = "relix";
const READ_ONLY_OPTIONS: &str = "-c default_transaction_read_only=on";

pub(crate) struct Connector {
    base: Config,
    configured_database: String,
    tls_mode: TlsMode,
    tls: Option<ServerNameTls>,
    timeout: Duration,
}

impl Connector {
    pub(crate) async fn new(
        flavor: Flavor,
        config: &ConnectionConfig,
    ) -> Result<Self, DriverError> {
        let settings = &config.settings;
        let user = non_empty(settings.username.as_deref()).unwrap_or(flavor.default_user());
        let configured_database = non_empty(settings.database.as_deref())
            .or(flavor.default_database())
            .unwrap_or(user)
            .to_owned();
        let timeout = settings.connect_timeout();
        let mut base = Config::new();
        base.host(config.host_or_default())
            .port(settings.port.unwrap_or(flavor.default_port()))
            .user(user)
            .application_name(APPLICATION_NAME)
            .connect_timeout(timeout);
        if let Some(password) = &config.password {
            base.password(password.expose());
        }
        if config.read_only {
            base.options(READ_ONLY_OPTIONS);
        }
        Ok(Self {
            base,
            configured_database,
            tls_mode: settings.tls.mode,
            tls: tls_connector(&settings.tls)
                .await?
                .map(|tls| ServerNameTls::new(tls, config.tls_server_name.clone())),
            timeout,
        })
    }

    pub(crate) fn configured_database(&self) -> &str {
        &self.configured_database
    }

    pub(crate) async fn open(&self, database: &str) -> Result<ManagedClient, DriverError> {
        let mut config = self.base.clone();
        config.dbname(database);
        with_timeout(self.timeout, async {
            match (&self.tls, self.tls_mode) {
                (Some(tls), TlsMode::Prefer) => {
                    config.ssl_mode(SslMode::Prefer);
                    match connect_with(&config, tls.clone()).await {
                        Err(error) if is_tls_failure(&error) => {
                            config.ssl_mode(SslMode::Disable);
                            connect_with(&config, NoTls).await
                        }
                        other => other,
                    }
                }
                (Some(tls), _) => {
                    config.ssl_mode(SslMode::Require);
                    connect_with(&config, tls.clone()).await
                }
                (None, _) => {
                    config.ssl_mode(SslMode::Disable);
                    connect_with(&config, NoTls).await
                }
            }
            .map_err(map_error)
        })
        .await
    }

    pub(crate) async fn cancel(&self, token: &CancelToken) -> Result<(), DriverError> {
        with_timeout(self.timeout, async {
            match &self.tls {
                Some(tls) => token.cancel_query(tls.clone()).await,
                None => token.cancel_query(NoTls).await,
            }
            .map_err(map_error)
        })
        .await
    }
}

async fn connect_with<T>(config: &Config, tls: T) -> Result<ManagedClient, tokio_postgres::Error>
where
    T: MakeTlsConnect<Socket>,
    T::Stream: Send + 'static,
    T::TlsConnect: Send,
    <T::TlsConnect as TlsConnect<Socket>>::Future: Send,
{
    let (client, connection) = config.connect(tls).await?;
    let connection_task = tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(ManagedClient::new(client, connection_task))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::{ConnectionSettings, TlsSettings};

    #[tokio::test]
    async fn database_defaults_to_user_name() {
        let settings = ConnectionSettings {
            username: Some("alice".into()),
            database: Some("  ".into()),
            tls: TlsSettings {
                mode: TlsMode::Disable,
                ca_certificate: None,
            },
            ..ConnectionSettings::default()
        };
        let connector = Connector::new(Flavor::Postgres, &ConnectionConfig::new(settings, None))
            .await
            .unwrap();
        assert_eq!(connector.configured_database(), "alice");
        assert_eq!(connector.base.get_ports(), &[5432]);
    }

    #[tokio::test]
    async fn cockroach_defaults_to_root_and_defaultdb() {
        let settings = ConnectionSettings {
            tls: TlsSettings {
                mode: TlsMode::Disable,
                ca_certificate: None,
            },
            ..ConnectionSettings::default()
        };
        let connector = Connector::new(Flavor::CockroachDb, &ConnectionConfig::new(settings, None))
            .await
            .unwrap();
        assert_eq!(connector.configured_database(), "defaultdb");
        assert_eq!(connector.base.get_user(), Some("root"));
        assert_eq!(connector.base.get_ports(), &[26257]);
    }

    #[tokio::test]
    async fn read_only_configs_start_read_only_sessions() {
        let settings = ConnectionSettings {
            tls: TlsSettings {
                mode: TlsMode::Disable,
                ca_certificate: None,
            },
            ..ConnectionSettings::default()
        };
        let config = ConnectionConfig::new(settings, None);
        let writable = Connector::new(Flavor::Postgres, &config).await.unwrap();
        assert_eq!(writable.base.get_options(), None);
        let read_only = Connector::new(Flavor::Postgres, &config.with_read_only(true))
            .await
            .unwrap();
        assert_eq!(read_only.base.get_options(), Some(READ_ONLY_OPTIONS));
    }
}
