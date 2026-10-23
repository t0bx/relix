/*
 * @author t0bx
 * @since 23.10.2026
 */

use mysql_async::SslOpts;
use relix_core::{TlsMode, TlsSettings};
use relix_driver_api::DriverError;
use std::path::Path;

/// Returns the TLS configurations to try in order; `None` stands for a plaintext attempt.
/// `Prefer` tries an unverified TLS session first and falls back to plaintext.
pub async fn tls_attempts(
    settings: &TlsSettings,
    server_name: Option<&str>,
) -> Result<Vec<Option<SslOpts>>, DriverError> {
    install_crypto_provider();
    Ok(match settings.mode {
        TlsMode::Disable => vec![None],
        TlsMode::Prefer => vec![Some(unverified()), None],
        TlsMode::Require => vec![Some(unverified())],
        TlsMode::VerifyFull => {
            let verified = verified(settings.ca_certificate.as_deref()).await?;
            vec![Some(match server_name {
                Some(name) => verified.with_danger_tls_hostname_override(Some(name.to_owned())),
                None => verified,
            })]
        }
    })
}

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn unverified() -> SslOpts {
    SslOpts::default()
        .with_danger_accept_invalid_certs(true)
        .with_danger_skip_domain_validation(true)
}

async fn verified(ca_certificate: Option<&Path>) -> Result<SslOpts, DriverError> {
    let Some(path) = ca_certificate else {
        return Ok(SslOpts::default());
    };
    let certificate = tokio::fs::read(path).await.map_err(|error| {
        DriverError::configuration(format!(
            "cannot read CA certificate {}: {error}",
            path.display()
        ))
    })?;
    Ok(SslOpts::default()
        .with_root_certs(vec![certificate.into()])
        .with_disable_built_in_roots(true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_driver_api::ErrorKind;
    use std::path::PathBuf;

    fn settings(mode: TlsMode, ca_certificate: Option<PathBuf>) -> TlsSettings {
        TlsSettings {
            mode,
            ca_certificate,
        }
    }

    #[tokio::test]
    async fn modes_produce_expected_attempts() {
        let disable = tls_attempts(&settings(TlsMode::Disable, None), None)
            .await
            .unwrap();
        assert_eq!(disable, vec![None]);
        let prefer = tls_attempts(&settings(TlsMode::Prefer, None), None)
            .await
            .unwrap();
        assert_eq!(prefer.len(), 2);
        assert!(
            prefer[0]
                .as_ref()
                .is_some_and(SslOpts::accept_invalid_certs)
        );
        assert!(prefer[1].is_none());
        let require = tls_attempts(&settings(TlsMode::Require, None), None)
            .await
            .unwrap();
        assert!(
            require[0]
                .as_ref()
                .is_some_and(SslOpts::skip_domain_validation)
        );
        let full = tls_attempts(&settings(TlsMode::VerifyFull, None), None)
            .await
            .unwrap();
        assert!(
            full[0]
                .as_ref()
                .is_some_and(|opts| !opts.accept_invalid_certs())
        );
    }

    #[tokio::test]
    async fn missing_ca_certificate_is_a_configuration_error() {
        let missing = settings(
            TlsMode::VerifyFull,
            Some("/nonexistent/relix-ca.pem".into()),
        );
        let error = tls_attempts(&missing, None).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::Configuration);
    }
}
