/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_connect_error;
use redis::{
    Client, ConnectionAddr, ConnectionInfo, IntoConnectionInfo, RedisConnectionInfo,
    TlsCertificates,
};
use relix_core::{TlsMode, TlsSettings};
use relix_driver_api::{DriverError, ErrorKind};
use std::path::Path;

pub fn uses_tls(mode: TlsMode) -> bool {
    matches!(mode, TlsMode::Require | TlsMode::VerifyFull)
}

/// Builds the redis client for the requested TLS mode. Redis has no opportunistic TLS
/// upgrade, so `Prefer` connects in plaintext exactly like `Disable`. `Require` encrypts
/// without verifying the certificate, `VerifyFull` verifies it against the system roots
/// or the configured CA certificate.
pub async fn build_client(
    host: &str,
    port: u16,
    tls: &TlsSettings,
    redis: RedisConnectionInfo,
) -> Result<Client, DriverError> {
    let address = match tls.mode {
        TlsMode::Disable | TlsMode::Prefer => ConnectionAddr::Tcp(host.to_owned(), port),
        TlsMode::Require | TlsMode::VerifyFull => {
            install_crypto_provider();
            ConnectionAddr::TcpTls {
                host: host.to_owned(),
                port,
                insecure: tls.mode == TlsMode::Require,
                tls_params: None,
            }
        }
    };
    let info = connection_info(address, redis)?;
    let tls_enabled = uses_tls(tls.mode);
    match (&tls.ca_certificate, tls.mode) {
        (Some(path), TlsMode::VerifyFull) => {
            let certificates = TlsCertificates {
                client_tls: None,
                root_cert: Some(read_certificate(path).await?),
            };
            Client::build_with_tls(info, certificates)
                .map_err(|error| map_connect_error(error, tls_enabled))
        }
        _ => Client::open(info).map_err(|error| map_connect_error(error, tls_enabled)),
    }
}

fn connection_info(
    address: ConnectionAddr,
    redis: RedisConnectionInfo,
) -> Result<ConnectionInfo, DriverError> {
    address
        .into_connection_info()
        .map(|info| info.set_redis_settings(redis))
        .map_err(|error| map_connect_error(error, false))
}

async fn read_certificate(path: &Path) -> Result<Vec<u8>, DriverError> {
    tokio::fs::read(path).await.map_err(|error| {
        DriverError::new(
            ErrorKind::Configuration,
            format!("cannot read CA certificate {}: {error}", path.display()),
        )
    })
}

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_require_and_verify_full_use_tls() {
        assert!(!uses_tls(TlsMode::Disable));
        assert!(!uses_tls(TlsMode::Prefer));
        assert!(uses_tls(TlsMode::Require));
        assert!(uses_tls(TlsMode::VerifyFull));
    }

    #[tokio::test]
    async fn prefer_builds_a_plaintext_client() {
        let settings = TlsSettings {
            mode: TlsMode::Prefer,
            ca_certificate: None,
        };
        let client = build_client("localhost", 6379, &settings, RedisConnectionInfo::default())
            .await
            .unwrap();
        assert!(matches!(
            client.get_connection_info().addr(),
            ConnectionAddr::Tcp(_, 6379)
        ));
    }

    #[tokio::test]
    async fn missing_ca_certificate_is_a_configuration_error() {
        let settings = TlsSettings {
            mode: TlsMode::VerifyFull,
            ca_certificate: Some("/nonexistent/relix-ca.pem".into()),
        };
        let error = build_client("localhost", 6379, &settings, RedisConnectionInfo::default())
            .await
            .err()
            .map(|error| error.kind);
        assert_eq!(error, Some(ErrorKind::Configuration));
    }
}
