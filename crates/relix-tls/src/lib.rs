/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{TlsMode, TlsSettings};
use relix_driver_api::{DriverError, ErrorKind};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use std::path::Path;
use std::sync::Arc;

/// Builds the rustls client configuration for the configured mode. `Prefer` and
/// `Require` encrypt without validating the certificate chain (like libpq);
/// `VerifyFull` validates the chain against the Mozilla roots plus an optional CA file
/// and checks the hostname.
pub async fn client_config(settings: &TlsSettings) -> Result<Option<ClientConfig>, DriverError> {
    let config = match settings.mode {
        TlsMode::Disable => return Ok(None),
        TlsMode::Prefer | TlsMode::Require => encryption_only_config()?,
        TlsMode::VerifyFull => {
            verifying_config(root_store(settings.ca_certificate.as_deref()).await?)?
        }
    };
    Ok(Some(config))
}

pub fn install_default_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub fn ring_provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn encryption_only_config() -> Result<ClientConfig, DriverError> {
    let provider = ring_provider();
    let verifier = Arc::new(UnverifiedServerCertificate {
        provider: provider.clone(),
    });
    Ok(ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(tls_error)?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth())
}

fn verifying_config(roots: RootCertStore) -> Result<ClientConfig, DriverError> {
    Ok(ClientConfig::builder_with_provider(ring_provider())
        .with_safe_default_protocol_versions()
        .map_err(tls_error)?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

async fn root_store(ca_certificate: Option<&Path>) -> Result<RootCertStore, DriverError> {
    let mut roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let Some(path) = ca_certificate else {
        return Ok(roots);
    };
    let pem = tokio::fs::read(path).await.map_err(|error| {
        DriverError::new(
            ErrorKind::Tls,
            format!("cannot read CA certificate {}: {error}", path.display()),
        )
    })?;
    let certificates = CertificateDer::pem_slice_iter(&pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            DriverError::new(
                ErrorKind::Tls,
                format!("invalid CA certificate {}: {error}", path.display()),
            )
        })?;
    if certificates.is_empty() {
        return Err(DriverError::new(
            ErrorKind::Tls,
            format!("no certificates found in {}", path.display()),
        ));
    }
    for certificate in certificates {
        roots.add(certificate).map_err(tls_error)?;
    }
    Ok(roots)
}

fn tls_error(error: rustls::Error) -> DriverError {
    DriverError::new(ErrorKind::Tls, error.to_string())
}

#[derive(Debug)]
struct UnverifiedServerCertificate {
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for UnverifiedServerCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            certificate,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            certificate,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[tokio::test]
    async fn disabled_mode_has_no_connector() {
        let settings = TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        };
        assert!(client_config(&settings).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn encrypting_modes_build_connectors() {
        for mode in [TlsMode::Prefer, TlsMode::Require, TlsMode::VerifyFull] {
            let settings = TlsSettings {
                mode,
                ca_certificate: None,
            };
            assert!(client_config(&settings).await.unwrap().is_some());
        }
    }

    #[tokio::test]
    async fn missing_ca_file_is_a_tls_error() {
        let settings = TlsSettings {
            mode: TlsMode::VerifyFull,
            ca_certificate: Some("/nonexistent/relix-ca.pem".into()),
        };
        let error = client_config(&settings).await.err().unwrap();
        assert_eq!(error.kind, ErrorKind::Tls);
    }
}
