/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::RedisConnection;
use crate::driver::DEFAULT_PORT;
use crate::errors::map_connect_error;
use crate::runner::CommandRunner;
use crate::server_details::{parse_info, redis_version};
use crate::tls::{build_client, uses_tls};
use redis::RedisConnectionInfo;
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use relix_core::ConnectionConfig;
use relix_driver_api::{DriverError, ErrorKind, ServerInfo, with_timeout};
use std::time::Duration;

const RECONNECT_ATTEMPTS: usize = 3;
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// Opens an auto-reconnecting connection and verifies it with `PING`. TLS modes follow
/// [`build_client`]: `Prefer` behaves like `Disable` because Redis cannot negotiate TLS
/// opportunistically on a plaintext port.
pub async fn open_connection(config: &ConnectionConfig) -> Result<RedisConnection, DriverError> {
    let settings = &config.settings;
    let database = parse_database_index(settings.database.as_deref())?;
    let port = settings.port.unwrap_or(DEFAULT_PORT);
    let redis = handshake_settings(config, database);
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
        let version = verify_server(&runner).await?;
        Ok(RedisConnection::new(
            runner,
            ServerInfo {
                product: "Redis".to_owned(),
                version,
            },
        ))
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

async fn verify_server(runner: &CommandRunner) -> Result<String, DriverError> {
    let mut pipeline = redis::pipe();
    pipeline.cmd("PING").cmd("INFO").arg("server");
    let (_, info): (String, String) = runner.query_pipeline(&pipeline).await?;
    Ok(redis_version(&parse_info(&info)).unwrap_or_else(|| "unknown".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
