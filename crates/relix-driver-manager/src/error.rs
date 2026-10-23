/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_config::ConfigError;
use relix_core::DriverId;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("offline mode is enabled, driver packages cannot be downloaded")]
    Offline,
    #[error("refusing insecure URL '{0}': only https:// sources are allowed")]
    InsecureUrl(String),
    #[error("registry '{registry}' is unavailable: {message}")]
    Registry { registry: String, message: String },
    #[error("no registry named '{0}' is configured")]
    UnknownRegistry(String),
    #[error("download failed: {0}")]
    Download(String),
    #[error("package is larger than the allowed {limit} bytes")]
    TooLarge { limit: u64 },
    #[error("package size mismatch: expected {expected} bytes, received {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("package checksum mismatch: expected {expected}, computed {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("package signature is invalid: {0}")]
    InvalidSignature(String),
    #[error("registry public key is invalid: {0}")]
    InvalidPublicKey(String),
    #[error("package index is invalid: {0}")]
    InvalidIndex(String),
    #[error("package is invalid: {0}")]
    InvalidPackage(String),
    #[error("driver is incompatible: {0}")]
    Incompatible(String),
    #[error("driver '{0}' is built in and cannot be installed or removed")]
    BuiltIn(DriverId),
    #[error("driver '{0}' is not installed")]
    NotInstalled(DriverId),
    #[error("another driver installation is in progress")]
    Busy,
    #[error("internal error: {0}")]
    Internal(String),
    #[error("failed to access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Config(#[from] ConfigError),
}

impl ManagerError {
    pub fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}
