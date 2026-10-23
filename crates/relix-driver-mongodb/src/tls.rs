/*
 * @author t0bx
 * @since 23.10.2026
 */

use mongodb::options::{Tls, TlsOptions};
use relix_core::{ConnectionConfig, TlsMode};
use relix_driver_api::{DriverError, ErrorKind};

/// Maps the TLS mode onto the MongoDB client. MongoDB cannot upgrade a plaintext
/// connection, so `Prefer` connects without TLS like `Disable`; `Require` encrypts
/// without validating the certificate and `VerifyFull` validates chain and hostname
/// against the Mozilla roots or the configured CA file. The client always verifies
/// against the host it dials and cannot be given a different server name, so
/// `VerifyFull` through a forwarded endpoint (SSH tunnel) is refused.
pub fn tls_options(config: &ConnectionConfig) -> Result<Tls, DriverError> {
    let settings = &config.settings.tls;
    match settings.mode {
        TlsMode::Disable | TlsMode::Prefer => Ok(Tls::Disabled),
        TlsMode::Require => {
            let options = TlsOptions::builder()
                .allow_invalid_certificates(true)
                .ca_file_path(settings.ca_certificate.clone())
                .build();
            Ok(Tls::Enabled(options))
        }
        TlsMode::VerifyFull => {
            ensure_verifiable_host(config)?;
            let options = TlsOptions::builder()
                .ca_file_path(settings.ca_certificate.clone())
                .build();
            Ok(Tls::Enabled(options))
        }
    }
}

pub fn ensure_verifiable_host(config: &ConnectionConfig) -> Result<(), DriverError> {
    let forwarded = config
        .tls_server_name
        .as_deref()
        .is_some_and(|name| name != config.host_or_default());
    if forwarded {
        return Err(DriverError::new(
            ErrorKind::Tls,
            "certificate verification through a forwarded endpoint is not supported by the \
             MongoDB client: use TLS mode 'require' over the SSH tunnel or a VPN instead",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::{ConnectionSettings, TlsSettings};

    fn config(mode: TlsMode) -> ConnectionConfig {
        let settings = ConnectionSettings {
            host: Some("db.internal".into()),
            tls: TlsSettings {
                mode,
                ca_certificate: None,
            },
            ..ConnectionSettings::default()
        };
        ConnectionConfig::new(settings, None)
    }

    #[test]
    fn prefer_and_disable_connect_in_plaintext() {
        assert!(matches!(
            tls_options(&config(TlsMode::Prefer)),
            Ok(Tls::Disabled)
        ));
        assert!(matches!(
            tls_options(&config(TlsMode::Disable)),
            Ok(Tls::Disabled)
        ));
    }

    #[test]
    fn require_skips_certificate_validation() {
        let Ok(Tls::Enabled(options)) = tls_options(&config(TlsMode::Require)) else {
            panic!("require must enable TLS");
        };
        assert_eq!(options.allow_invalid_certificates, Some(true));
    }

    #[test]
    fn verification_through_a_tunnel_is_refused() {
        let direct = config(TlsMode::VerifyFull);
        assert!(matches!(tls_options(&direct), Ok(Tls::Enabled(_))));
        let tunneled = direct.redirected("127.0.0.1", 40000);
        let error = tls_options(&tunneled).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Tls);
        let require = config(TlsMode::Require).redirected("127.0.0.1", 40000);
        assert!(tls_options(&require).is_ok());
    }
}
