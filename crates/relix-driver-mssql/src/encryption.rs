/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{TlsMode, TlsSettings};
use relix_driver_api::{DriverError, ErrorKind};
use std::path::{Path, PathBuf};
use tiberius::{Config, EncryptionLevel};

const SUPPORTED_CA_EXTENSIONS: [&str; 3] = ["pem", "crt", "der"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CertificateTrust {
    AnyCertificate,
    Validated { ca_certificate: Option<PathBuf> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EncryptionAttempt {
    pub(crate) level: EncryptionLevel,
    pub(crate) trust: CertificateTrust,
}

impl EncryptionAttempt {
    fn new(level: EncryptionLevel, trust: CertificateTrust) -> Self {
        Self { level, trust }
    }

    pub(crate) fn apply(&self, config: &mut Config) {
        config.encryption(self.level);
        match &self.trust {
            CertificateTrust::AnyCertificate => config.trust_cert(),
            CertificateTrust::Validated {
                ca_certificate: Some(path),
            } => config.trust_cert_ca(path.display()),
            CertificateTrust::Validated {
                ca_certificate: None,
            } => {}
        }
    }
}

/// Returns the TDS encryption settings to try in order. `Prefer` first asks for a fully
/// encrypted session without validating the certificate and falls back to plaintext;
/// `VerifyFull` validates against the OS trust store or the configured CA file. A server
/// that enforces encryption still gets an encrypted session when TLS is disabled here.
pub(crate) fn encryption_attempts(
    settings: &TlsSettings,
) -> Result<Vec<EncryptionAttempt>, DriverError> {
    let unverified = |level| EncryptionAttempt::new(level, CertificateTrust::AnyCertificate);
    Ok(match settings.mode {
        TlsMode::Disable => vec![unverified(EncryptionLevel::NotSupported)],
        TlsMode::Prefer => vec![
            unverified(EncryptionLevel::Required),
            unverified(EncryptionLevel::NotSupported),
        ],
        TlsMode::Require => vec![unverified(EncryptionLevel::Required)],
        TlsMode::VerifyFull => {
            if let Some(path) = &settings.ca_certificate {
                check_ca_certificate(path)?;
            }
            vec![EncryptionAttempt::new(
                EncryptionLevel::Required,
                CertificateTrust::Validated {
                    ca_certificate: settings.ca_certificate.clone(),
                },
            )]
        }
    })
}

fn check_ca_certificate(path: &Path) -> Result<(), DriverError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !SUPPORTED_CA_EXTENSIONS.contains(&extension.as_str()) {
        return Err(DriverError::new(
            ErrorKind::Tls,
            format!(
                "unsupported CA certificate file {}: SQL Server connections need a single \
                 certificate in a .pem, .crt or .der file",
                path.display()
            ),
        ));
    }
    if !path.is_file() {
        return Err(DriverError::new(
            ErrorKind::Tls,
            format!("cannot read CA certificate {}", path.display()),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn settings(mode: TlsMode, ca_certificate: Option<PathBuf>) -> TlsSettings {
        TlsSettings {
            mode,
            ca_certificate,
        }
    }

    #[test]
    fn modes_map_to_tds_encryption_levels() {
        let levels = |mode| {
            encryption_attempts(&settings(mode, None))
                .unwrap()
                .into_iter()
                .map(|attempt| attempt.level)
                .collect::<Vec<_>>()
        };
        assert_eq!(levels(TlsMode::Disable), [EncryptionLevel::NotSupported]);
        assert_eq!(
            levels(TlsMode::Prefer),
            [EncryptionLevel::Required, EncryptionLevel::NotSupported]
        );
        assert_eq!(levels(TlsMode::Require), [EncryptionLevel::Required]);
        assert_eq!(levels(TlsMode::VerifyFull), [EncryptionLevel::Required]);
    }

    #[test]
    fn only_verify_full_validates_certificates() {
        let require = encryption_attempts(&settings(TlsMode::Require, None)).unwrap();
        assert_eq!(require[0].trust, CertificateTrust::AnyCertificate);
        let full = encryption_attempts(&settings(TlsMode::VerifyFull, None)).unwrap();
        assert_eq!(
            full[0].trust,
            CertificateTrust::Validated {
                ca_certificate: None
            }
        );
    }

    #[test]
    fn unusable_ca_files_are_tls_errors() {
        let bundle = settings(TlsMode::VerifyFull, Some("/tmp/relix-ca.p12".into()));
        assert_eq!(
            encryption_attempts(&bundle).unwrap_err().kind,
            ErrorKind::Tls
        );
        let missing = settings(
            TlsMode::VerifyFull,
            Some("/nonexistent/relix-ca.pem".into()),
        );
        assert_eq!(
            encryption_attempts(&missing).unwrap_err().kind,
            ErrorKind::Tls
        );
    }
}
