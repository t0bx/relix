/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ManagerError;
use crate::index::{PackageArtifact, PackageRelease};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

pub const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
pub const SIGNATURE_DOMAIN: &str = "relix-package-v1";

/// The exact bytes a registry signs for one artifact. Binding the driver API version and
/// size (not only the checksum) prevents a tampered index from re-labelling a genuine
/// package as compatible with a different RELIX build.
pub fn signing_message(release: &PackageRelease, artifact: &PackageArtifact) -> String {
    format!(
        "{SIGNATURE_DOMAIN}\n{}\n{}\n{}\n{}\n{}\n{}",
        release.id,
        release.version,
        release.api_version,
        artifact.target,
        artifact.size,
        artifact.sha256.to_ascii_lowercase()
    )
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Parses a registry public key: the 32 raw ed25519 key bytes, base64 encoded (standard
/// alphabet, with padding).
pub fn parse_public_key(encoded: &str) -> Result<VerifyingKey, ManagerError> {
    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|error| ManagerError::InvalidPublicKey(error.to_string()))?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| ManagerError::InvalidPublicKey("expected 32 key bytes".to_owned()))?;
    VerifyingKey::from_bytes(&bytes)
        .map_err(|error| ManagerError::InvalidPublicKey(error.to_string()))
}

pub fn verify_package(
    bytes: &[u8],
    release: &PackageRelease,
    artifact: &PackageArtifact,
    key: &VerifyingKey,
) -> Result<(), ManagerError> {
    let actual_size = bytes.len() as u64;
    if actual_size > MAX_PACKAGE_BYTES {
        return Err(ManagerError::TooLarge {
            limit: MAX_PACKAGE_BYTES,
        });
    }
    if actual_size != artifact.size {
        return Err(ManagerError::SizeMismatch {
            expected: artifact.size,
            actual: actual_size,
        });
    }
    let actual_sha256 = sha256_hex(bytes);
    if !actual_sha256.eq_ignore_ascii_case(&artifact.sha256) {
        return Err(ManagerError::ChecksumMismatch {
            expected: artifact.sha256.clone(),
            actual: actual_sha256,
        });
    }
    let signature = decode_signature(&artifact.signature)?;
    key.verify_strict(signing_message(release, artifact).as_bytes(), &signature)
        .map_err(|_| {
            ManagerError::InvalidSignature(
                "it was not produced by the registry's signing key".to_owned(),
            )
        })
}

fn decode_signature(encoded: &str) -> Result<Signature, ManagerError> {
    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|error| ManagerError::InvalidSignature(error.to_string()))?;
    Signature::from_slice(&bytes).map_err(|error| ManagerError::InvalidSignature(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use relix_core::DriverId;
    use semver::Version;

    fn package(bytes: &[u8], key: &SigningKey) -> (PackageRelease, PackageArtifact) {
        let mut release = PackageRelease {
            id: DriverId::new("csv"),
            name: "CSV".into(),
            version: Version::new(1, 0, 0),
            api_version: 1,
            description: String::new(),
            artifacts: Vec::new(),
        };
        let mut artifact = PackageArtifact {
            target: "linux-x86_64".into(),
            url: "https://example.test/csv.tar.gz".into(),
            size: bytes.len() as u64,
            sha256: sha256_hex(bytes),
            signature: String::new(),
        };
        artifact.signature = STANDARD.encode(
            key.sign(signing_message(&release, &artifact).as_bytes())
                .to_bytes(),
        );
        release.artifacts.push(artifact.clone());
        (release, artifact)
    }

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn genuine_packages_verify() {
        let (release, artifact) = package(b"payload", &key(1));
        verify_package(b"payload", &release, &artifact, &key(1).verifying_key()).unwrap();
    }

    #[test]
    fn size_checksum_and_signature_failures_are_distinguished() {
        let (release, artifact) = package(b"payload", &key(1));
        let verifying = key(1).verifying_key();
        assert!(matches!(
            verify_package(b"payload!", &release, &artifact, &verifying),
            Err(ManagerError::SizeMismatch { .. })
        ));
        assert!(matches!(
            verify_package(b"PAYLOAD", &release, &artifact, &verifying),
            Err(ManagerError::ChecksumMismatch { .. })
        ));
        assert!(matches!(
            verify_package(b"payload", &release, &artifact, &key(2).verifying_key()),
            Err(ManagerError::InvalidSignature(_))
        ));
        let mut relabelled = release.clone();
        relabelled.api_version = 99;
        assert!(matches!(
            verify_package(b"payload", &relabelled, &artifact, &verifying),
            Err(ManagerError::InvalidSignature(_))
        ));
    }

    #[test]
    fn public_keys_are_base64_encoded_raw_bytes() {
        let encoded = STANDARD.encode(key(3).verifying_key().to_bytes());
        assert_eq!(parse_public_key(&encoded).unwrap(), key(3).verifying_key());
        assert!(parse_public_key("not base64!").is_err());
        assert!(parse_public_key(&STANDARD.encode([1u8; 16])).is_err());
    }
}
