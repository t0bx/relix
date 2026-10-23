/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::ids::ServerConfigId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    #[default]
    Direct,
    SshTunnel,
    Vpn,
    VpnSshTunnel,
}

impl NetworkMode {
    pub const ALL: [NetworkMode; 4] =
        [Self::Direct, Self::SshTunnel, Self::Vpn, Self::VpnSshTunnel];

    pub fn label(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::SshTunnel => "SSH tunnel",
            Self::Vpn => "VPN",
            Self::VpnSshTunnel => "VPN + SSH tunnel",
        }
    }

    pub fn uses_ssh(self) -> bool {
        matches!(self, Self::SshTunnel | Self::VpnSshTunnel)
    }

    pub fn uses_vpn(self) -> bool {
        matches!(self, Self::Vpn | Self::VpnSshTunnel)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkSettings {
    pub mode: NetworkMode,
    pub ssh: Option<ServerConfigId>,
    pub vpn: Option<ServerConfigId>,
    pub local_port: Option<u16>,
}

impl NetworkSettings {
    pub fn is_direct(&self) -> bool {
        self.mode == NetworkMode::Direct
    }

    pub fn ssh_config(&self) -> Option<ServerConfigId> {
        self.ssh.filter(|_| self.mode.uses_ssh())
    }

    pub fn vpn_profile(&self) -> Option<ServerConfigId> {
        self.vpn.filter(|_| self.mode.uses_vpn())
    }

    pub fn references(&self, id: ServerConfigId) -> bool {
        self.ssh_config() == Some(id) || self.vpn_profile() == Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_follow_the_selected_mode() {
        let ssh = ServerConfigId::new();
        let vpn = ServerConfigId::new();
        let mut settings = NetworkSettings {
            mode: NetworkMode::SshTunnel,
            ssh: Some(ssh),
            vpn: Some(vpn),
            local_port: None,
        };
        assert_eq!(settings.ssh_config(), Some(ssh));
        assert_eq!(settings.vpn_profile(), None);
        assert!(!settings.references(vpn));
        settings.mode = NetworkMode::VpnSshTunnel;
        assert!(settings.references(vpn) && settings.references(ssh));
        settings.mode = NetworkMode::Direct;
        assert!(settings.is_direct() && !settings.references(ssh));
    }
}
