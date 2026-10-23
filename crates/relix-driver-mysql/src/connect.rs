/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog_rows::{number_at, text_at};
use crate::connection::MySqlConnection;
use crate::errors::map_error;
use crate::flavor::Flavor;
use crate::tls::tls_attempts;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, OptsBuilder, Pool, PoolConstraints, PoolOpts, Row};
use relix_core::ConnectionConfig;
use relix_driver_api::{DriverError, ErrorKind, with_timeout};
use tokio::sync::Mutex;

/// `CONNECTION_ID()` is used for `KILL QUERY` because TiDB's 64-bit global connection ids
/// do not fit the 32-bit id of the protocol handshake.
const SESSION_IDENTITY_SQL: &str = "SELECT VERSION(), CONNECTION_ID()";
const POOL_MIN_CONNECTIONS: usize = 1;
const POOL_MAX_CONNECTIONS: usize = 4;

pub async fn connect(
    flavor: Flavor,
    config: &ConnectionConfig,
) -> Result<MySqlConnection, DriverError> {
    with_timeout(config.settings.connect_timeout(), establish(flavor, config)).await
}

/// Opens the session connection, trying each TLS attempt in order. Errors that may stem
/// from the TLS negotiation allow the next attempt; the pool reuses the options that worked.
async fn establish(
    flavor: Flavor,
    config: &ConnectionConfig,
) -> Result<MySqlConnection, DriverError> {
    let base = base_options(flavor, config);
    let mut last_error = None;
    for ssl in tls_attempts(&config.settings.tls, config.tls_server_name.as_deref()).await? {
        let opts: Opts = base.clone().ssl_opts(ssl).into();
        match Conn::new(opts.clone()).await {
            Ok(session) => return finish(flavor, config, opts, session).await,
            Err(error) => {
                let mapped = map_error(error);
                if !allows_next_attempt(mapped.kind) {
                    return Err(mapped);
                }
                last_error = Some(mapped);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| DriverError::internal("no connection attempt was made")))
}

async fn finish(
    flavor: Flavor,
    config: &ConnectionConfig,
    opts: Opts,
    mut session: Conn,
) -> Result<MySqlConnection, DriverError> {
    let row: Option<Row> = session
        .query_first(SESSION_IDENTITY_SQL)
        .await
        .map_err(map_error)?;
    let version = row
        .as_ref()
        .and_then(|row| text_at(row, 0))
        .unwrap_or_default();
    let session_id = row
        .as_ref()
        .and_then(|row| number_at(row, 1))
        .unwrap_or_else(|| u64::from(session.id()));
    Ok(MySqlConnection {
        server_flavor: Flavor::detect(&version),
        server_info: flavor.server_info(&version),
        session: Mutex::new(Some(session)),
        session_id,
        pool: Pool::new(opts),
        default_database: configured_database(config),
    })
}

fn base_options(flavor: Flavor, config: &ConnectionConfig) -> OptsBuilder {
    let settings = &config.settings;
    let pool_opts = PoolOpts::default();
    let pool_opts = PoolConstraints::new(POOL_MIN_CONNECTIONS, POOL_MAX_CONNECTIONS)
        .map_or(pool_opts.clone(), |constraints| {
            pool_opts.with_constraints(constraints)
        });
    OptsBuilder::default()
        .ip_or_hostname(config.host_or_default())
        .tcp_port(settings.port.unwrap_or(flavor.default_port()))
        .user(settings.username.clone().filter(|user| !user.is_empty()))
        .pass(
            config
                .password
                .as_ref()
                .map(|secret| secret.expose().to_owned()),
        )
        .db_name(configured_database(config))
        .client_found_rows(true)
        .prefer_socket(false)
        .pool_opts(pool_opts)
}

fn configured_database(config: &ConnectionConfig) -> Option<String> {
    config
        .settings
        .database
        .clone()
        .filter(|database| !database.trim().is_empty())
}

fn allows_next_attempt(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::Tls | ErrorKind::Connection | ErrorKind::Protocol | ErrorKind::Io
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::{ConnectionSettings, Secret};

    #[test]
    fn options_use_defaults_and_found_rows() {
        let settings = ConnectionSettings {
            username: Some("relix".into()),
            database: Some(" ".into()),
            ..ConnectionSettings::default()
        };
        let config = ConnectionConfig::new(settings, Some(Secret::new("pw")));
        let opts: Opts = base_options(Flavor::MySql, &config).into();
        assert_eq!(opts.ip_or_hostname(), "localhost");
        assert_eq!(opts.tcp_port(), 3306);
        assert_eq!(opts.user(), Some("relix"));
        assert_eq!(opts.db_name(), None);
        assert!(opts.client_found_rows());
        assert!(!opts.prefer_socket());
        assert_eq!(opts.pool_opts().constraints().max(), POOL_MAX_CONNECTIONS);
    }

    #[test]
    fn tidb_defaults_to_its_own_port() {
        let config = ConnectionConfig::new(ConnectionSettings::default(), None);
        let opts: Opts = base_options(Flavor::TiDb, &config).into();
        assert_eq!(opts.tcp_port(), 4000);
    }

    #[test]
    fn only_transport_failures_allow_fallback() {
        assert!(allows_next_attempt(ErrorKind::Tls));
        assert!(allows_next_attempt(ErrorKind::Connection));
        assert!(!allows_next_attempt(ErrorKind::Authentication));
    }
}
