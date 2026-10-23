/*
 * @author t0bx
 * @since 23.10.2026
 */

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use relix_core::Secret;
use relix_driver_api::{DriverError, ErrorKind};
use rsa::RsaPrivateKey;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::{DecodePrivateKey, EncodePublicKey};
use sha2::{Digest, Sha256};
use std::path::Path;

const ENCRYPTED_PKCS8: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----";
const PKCS1: &str = "-----BEGIN RSA PRIVATE KEY-----";
const LEGACY_ENCRYPTION: &str = "Proc-Type: 4,ENCRYPTED";

pub(crate) async fn read_private_key(
    path: &Path,
    passphrase: Option<&Secret>,
) -> Result<RsaPrivateKey, DriverError> {
    let pem = tokio::fs::read_to_string(path).await.map_err(|error| {
        DriverError::configuration(format!(
            "cannot read private key file {}: {error}",
            path.display()
        ))
    })?;
    parse_private_key(&pem, passphrase)
}

/// Parses an RSA private key in PKCS#8 (plain or PBES2-encrypted, as produced by
/// `openssl pkcs8 -topk8`) or unencrypted PKCS#1 PEM form. The passphrase is only used
/// for encrypted keys and never appears in error messages.
pub(crate) fn parse_private_key(
    pem: &str,
    passphrase: Option<&Secret>,
) -> Result<RsaPrivateKey, DriverError> {
    if pem.contains(ENCRYPTED_PKCS8) {
        let passphrase = passphrase.ok_or_else(|| {
            DriverError::new(
                ErrorKind::Authentication,
                "the private key is encrypted; enter its passphrase as the password",
            )
        })?;
        return RsaPrivateKey::from_pkcs8_encrypted_pem(pem, passphrase.expose().as_bytes())
            .map_err(|_| {
                DriverError::new(
                    ErrorKind::Authentication,
                    "cannot decrypt the private key: wrong passphrase or unsupported encryption",
                )
                .with_detail("supported: PKCS#8 with PBES2 (AES or 3DES, PBKDF2-SHA256 or scrypt)")
            });
    }
    if pem.contains(PKCS1) {
        if pem.contains(LEGACY_ENCRYPTION) {
            return Err(DriverError::configuration(
                "legacy encrypted PEM keys are not supported; convert the key with \
                 `openssl pkcs8 -topk8 -v2 aes-256-cbc`",
            ));
        }
        return RsaPrivateKey::from_pkcs1_pem(pem).map_err(invalid_key);
    }
    RsaPrivateKey::from_pkcs8_pem(pem).map_err(invalid_key)
}

/// Fingerprint Snowflake shows as `RSA_PUBLIC_KEY_FP`: the SHA-256 digest of the
/// DER-encoded public key (SubjectPublicKeyInfo), base64 encoded.
pub(crate) fn public_key_fingerprint(key: &RsaPrivateKey) -> Result<String, DriverError> {
    let der = key
        .to_public_key()
        .to_public_key_der()
        .map_err(|error| DriverError::internal(format!("cannot encode public key: {error}")))?;
    Ok(format!(
        "SHA256:{}",
        STANDARD.encode(Sha256::digest(der.as_bytes()))
    ))
}

fn invalid_key(error: impl std::fmt::Display) -> DriverError {
    DriverError::configuration(format!("invalid RSA private key: {error}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn missing_passphrase_for_encrypted_keys_asks_for_credentials() {
        let pem = format!("{ENCRYPTED_PKCS8}\nAAAA\n-----END ENCRYPTED PRIVATE KEY-----\n");
        let error = parse_private_key(&pem, None).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Authentication);
        let wrong = parse_private_key(&pem, Some(&Secret::new("hunter2"))).unwrap_err();
        assert!(!wrong.to_string().contains("hunter2"));
    }

    #[test]
    fn garbage_is_a_configuration_error() {
        let error = parse_private_key("not a key", None).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Configuration);
    }
}
