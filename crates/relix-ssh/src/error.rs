/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    Password,
    Passphrase,
}

impl CredentialKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Passphrase => "key passphrase",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyPrompt {
    pub server: String,
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String,
    pub public_key: String,
    pub known_hosts: PathBuf,
    pub trust_allowed: bool,
}

impl fmt::Display for HostKeyPrompt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the {} host key of {}:{} is not known ({})",
            self.algorithm, self.host, self.port, self.fingerprint
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SshError {
    #[error("invalid SSH configuration: {0}")]
    Configuration(String),
    #[error("cannot reach SSH server {endpoint}: {message}")]
    Connection { endpoint: String, message: String },
    #[error("SSH connection to {endpoint} timed out after {seconds}s")]
    Timeout { endpoint: String, seconds: u64 },
    #[error("SSH authentication as '{user}' on {endpoint} failed")]
    Authentication { endpoint: String, user: String },
    #[error("{} for SSH server '{server}' is required", kind.label())]
    CredentialsRequired {
        server: String,
        kind: CredentialKind,
    },
    #[error("{0}")]
    HostKeyUnknown(Box<HostKeyPrompt>),
    #[error(
        "HOST KEY MISMATCH for {endpoint}: the server presented {fingerprint}, which differs \
         from the key recorded in {} line {line}; the connection was refused",
        known_hosts.display()
    )]
    HostKeyMismatch {
        endpoint: String,
        fingerprint: String,
        known_hosts: PathBuf,
        line: usize,
    },
    #[error("SSH agent unavailable: {0}")]
    Agent(String),
    #[error("cannot use private key {}: {message}", path.display())]
    Key { path: PathBuf, message: String },
    #[error("SSH tunnel to {target} failed: {message}")]
    Forwarding { target: String, message: String },
    #[error("local address {address}:{port} is unavailable: {message}")]
    PortUnavailable {
        address: String,
        port: u16,
        message: String,
    },
    #[error("the SSH session was closed")]
    Closed,
}

impl SshError {
    pub fn connection(endpoint: &str, message: impl fmt::Display) -> Self {
        Self::Connection {
            endpoint: endpoint.to_owned(),
            message: message.to_string(),
        }
    }

    pub fn is_authentication(&self) -> bool {
        matches!(
            self,
            Self::Authentication { .. } | Self::CredentialsRequired { .. }
        )
    }

    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Connection { .. } | Self::Timeout { .. } | Self::Closed
        )
    }
}
