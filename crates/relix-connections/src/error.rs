/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};
use relix_project::ReferenceError;
use relix_ssh::SshError;
use relix_vpn::VpnError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectionError {
    #[error(transparent)]
    Driver(#[from] DriverError),
    #[error("SSH: {0}")]
    Ssh(#[from] SshError),
    #[error("VPN: {0}")]
    Vpn(#[from] VpnError),
    #[error("network configuration: {0}")]
    Reference(#[from] ReferenceError),
    #[error("{0}")]
    UnsupportedTransport(String),
}

impl ConnectionError {
    pub fn stage(&self) -> &'static str {
        match self {
            Self::Driver(_) => "database",
            Self::Ssh(_) => "SSH tunnel",
            Self::Vpn(_) => "VPN",
            Self::Reference(_) | Self::UnsupportedTransport(_) => "configuration",
        }
    }

    pub fn into_driver_error(self) -> DriverError {
        match self {
            Self::Driver(error) => error,
            other => {
                let kind = match &other {
                    Self::Ssh(error) if error.is_transient() => ErrorKind::Connection,
                    Self::Vpn(VpnError::Timeout { .. }) => ErrorKind::Timeout,
                    Self::Vpn(VpnError::Unreachable { .. } | VpnError::InterfaceDown { .. }) => {
                        ErrorKind::Connection
                    }
                    _ => ErrorKind::Configuration,
                };
                DriverError::new(kind, other.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_failures_never_look_like_database_authentication() {
        let ssh = ConnectionError::Ssh(SshError::Authentication {
            endpoint: "bastion:22".into(),
            user: "deploy".into(),
        });
        assert_eq!(ssh.stage(), "SSH tunnel");
        assert_eq!(ssh.into_driver_error().kind, ErrorKind::Configuration);
        let lost = ConnectionError::Ssh(SshError::Closed).into_driver_error();
        assert_eq!(lost.kind, ErrorKind::Connection);
    }
}
