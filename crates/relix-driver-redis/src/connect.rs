/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::RedisConnection;
use crate::driver::DEFAULT_PORT;
use crate::errors::map_connect_error;
use crate::flavor::Flavor;
use crate::runner::CommandRunner;
use crate::server_details::parse_info;
use crate::tls::{build_client, uses_tls};
use redis::RedisConnectionInfo;
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use relix_core::{ConnectionConfig, TlsMode};
use relix_driver_api::{DriverError, ErrorKind, ServerInfo, with_timeout};
use std::time::Duration;

const RECONNECT_ATTEMPTS: usize = 3;
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// Opens an auto-reconnecting connection and verifies it with `PING`. TLS modes follow
/// [`build_client`]: `Prefer` behaves like `Disable` because Redis cannot negotiate TLS
/// opportunistically on a plaintext port.
pub async fn open_connection(
    flavor: Flavor,
    config: &ConnectionConfig,
) -> Result<RedisConnection, DriverError> {
    let settings = &config.settings;
    let database = parse_database_index(settings.database.as_deref())?;
    let port = settings.port.unwrap_or(DEFAULT_PORT);
    let redis = handshake_settings(config, database);
    if settings.tls.mode == TlsMode::VerifyFull
        && config
            .tls_server_name
            .as_deref()
            .is_some_and(|name| name != config.host_or_default())
    {
        return Err(DriverError::new(
            ErrorKind::Tls,
            "certificate verification through a forwarded endpoint is not supported by the \
             Redis client: use TLS mode 'require' over the SSH tunnel or a VPN instead",
        ));
    }
    let client = build_client(config.host_or_default(), port, &settings.tls, redis).await?;
    let tls_enabled = uses_tls(settings.tls.mode);
    let manager_config = ConnectionManagerConfig::new()
        .set_connection_timeout(Some(settings.connect_timeout()))
        .set_response_timeout(None)
        .set_number_of_retries(RECONNECT_ATTEMPTS)
        .set_max_delay(MAX_RECONNECT_DELAY);
    let connecting = with_timeout(settings.connect_timeout(), async move {
        let manager = ConnectionManager::new_with_config(client, manager_config)
            .await
            .map_err(|error| map_connect_error(error, tls_enabled))?;
        let runner = CommandRunner::new(manager);
        let server_info = verify_server(&runner, flavor).await?;
        Ok(RedisConnection::new(runner, server_info))
    });
    connecting
        .await
        .map_err(|error| explain_tls_timeout(error, tls_enabled))
}

fn explain_tls_timeout(error: DriverError, tls_enabled: bool) -> DriverError {
    if tls_enabled && error.kind == ErrorKind::Timeout && error.detail.is_none() {
        error.with_detail(
            "the TLS handshake did not complete; check that the server has TLS enabled",
        )
    } else {
        error
    }
}

pub fn parse_database_index(database: Option<&str>) -> Result<i64, DriverError> {
    let Some(database) = database.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(0);
    };
    database
        .parse::<i64>()
        .ok()
        .filter(|index| *index >= 0)
        .ok_or_else(|| {
            DriverError::configuration(format!(
                "Redis database must be a non-negative number, got \"{database}\""
            ))
        })
}

fn handshake_settings(config: &ConnectionConfig, database: i64) -> RedisConnectionInfo {
    let redis = RedisConnectionInfo::default().set_db(database);
    let Some(password) = &config.password else {
        return redis;
    };
    let redis = redis.set_password(password.expose());
    match config.settings.username.as_deref().map(str::trim) {
        Some(username) if !username.is_empty() => redis.set_username(username),
        _ => redis,
    }
}

async fn verify_server(runner: &CommandRunner, flavor: Flavor) -> Result<ServerInfo, DriverError> {
    let mut pipeline = redis::pipe();
    pipeline.cmd("PING").cmd("INFO").arg("server");
    let (_, info): (String, String) = runner.query_pipeline(&pipeline).await?;
    let details = parse_info(&info);
    let server = Flavor::detect(&details);
    let version = server
        .version(&details)
        .unwrap_or_else(|| "unknown".to_owned());
    Ok(flavor.server_info(server, &version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn verified_tls_through_a_forwarded_endpoint_is_refused() {
        let settings = relix_core::ConnectionSettings {
            host: Some("cache.internal".into()),
            port: Some(6379),
            tls: relix_core::TlsSettings {
                mode: TlsMode::VerifyFull,
                ca_certificate: None,
            },
            ..relix_core::ConnectionSettings::default()
        };
        let tunneled = ConnectionConfig::new(settings, None).redirected("127.0.0.1", 40000);
        let Err(error) = open_connection(Flavor::Redis, &tunneled).await else {
            panic!("verification against a forwarded endpoint must not be attempted");
        };
        assert_eq!(error.kind, ErrorKind::Tls);
    }

    #[test]
    fn database_index_defaults_to_zero() {
        assert_eq!(parse_database_index(None).unwrap(), 0);
        assert_eq!(parse_database_index(Some("  ")).unwrap(), 0);
        assert_eq!(parse_database_index(Some("7")).unwrap(), 7);
    }

    #[test]
    fn non_numeric_database_is_a_configuration_error() {
        for invalid in ["main", "-1", "1.5"] {
            let error = parse_database_index(Some(invalid)).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Configuration);
        }
    }
}
