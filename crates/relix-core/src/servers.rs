/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::ids::ServerConfigId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigScope {
    Global,
    Project,
}

impl ConfigScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Global => Self::Project,
            Self::Project => Self::Global,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshAuthMethod {
    Password,
    #[default]
    PrivateKey,
    Agent,
}

impl SshAuthMethod {
    pub const ALL: [SshAuthMethod; 3] = [Self::PrivateKey, Self::Password, Self::Agent];

    pub fn label(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::PrivateKey => "private key",
            Self::Agent => "SSH agent",
        }
    }

    pub fn uses_secret(self) -> bool {
        !matches!(self, Self::Agent)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKeyPolicy {
    #[default]
    AskOnFirstUse,
    Strict,
}

impl HostKeyPolicy {
    pub const ALL: [HostKeyPolicy; 2] = [Self::AskOnFirstUse, Self::Strict];

    pub fn label(self) -> &'static str {
        match self {
            Self::AskOnFirstUse => "ask on first use",
            Self::Strict => "known hosts only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReconnectPolicy {
    pub enabled: bool,
    pub max_attempts: u32,
    pub delay_secs: u64,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_attempts: 3,
            delay_secs: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshServerConfig {
    pub id: ServerConfigId,
    pub name: String,
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub auth: SshAuthMethod,
    #[serde(default)]
    pub private_key: Option<PathBuf>,
    #[serde(default)]
    pub save_secret: bool,
    #[serde(default)]
    pub use_ssh_config: bool,
    #[serde(default)]
    pub known_hosts_file: Option<PathBuf>,
    #[serde(default)]
    pub host_key_policy: HostKeyPolicy,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_secs: u64,
    #[serde(default = "default_keepalive")]
    pub keepalive_interval_secs: u64,
    #[serde(default)]
    pub jump_host: Option<ServerConfigId>,
    #[serde(default = "default_bind_address")]
    pub local_bind_address: String,
    #[serde(default)]
    pub reconnect: ReconnectPolicy,
}

impl SshServerConfig {
    pub fn new(name: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            id: ServerConfigId::new(),
            name: name.into(),
            host: host.into(),
            port: default_ssh_port(),
            username: String::new(),
            auth: SshAuthMethod::default(),
            private_key: None,
            save_secret: false,
            use_ssh_config: false,
            known_hosts_file: None,
            host_key_policy: HostKeyPolicy::default(),
            connect_timeout_secs: default_connect_timeout(),
            keepalive_interval_secs: default_keepalive(),
            jump_host: None,
            local_bind_address: default_bind_address(),
            reconnect: ReconnectPolicy::default(),
        }
    }

    pub fn connect_timeout(&self) -> Duration {
        Duration::from_secs(self.connect_timeout_secs.max(1))
    }

    pub fn keepalive_interval(&self) -> Option<Duration> {
        (self.keepalive_interval_secs > 0)
            .then(|| Duration::from_secs(self.keepalive_interval_secs))
    }

    pub fn endpoint_label(&self) -> String {
        if self.username.is_empty() {
            format!("{}:{}", self.host, self.port)
        } else {
            format!("{}@{}:{}", self.username, self.host, self.port)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VpnMode {
    #[default]
    Existing,
    Managed,
}

impl VpnMode {
    pub const ALL: [VpnMode; 2] = [Self::Existing, Self::Managed];

    pub fn label(self) -> &'static str {
        match self {
            Self::Existing => "existing system VPN",
            Self::Managed => "managed by Relix",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VpnProvider {
    #[default]
    WireGuard,
    OpenVpn,
    Other,
}

impl VpnProvider {
    pub const ALL: [VpnProvider; 3] = [Self::WireGuard, Self::OpenVpn, Self::Other];

    pub fn label(self) -> &'static str {
        match self {
            Self::WireGuard => "WireGuard",
            Self::OpenVpn => "OpenVPN",
            Self::Other => "other",
        }
    }

    pub fn is_manageable(self) -> bool {
        !matches!(self, Self::Other)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VpnProfile {
    pub id: ServerConfigId,
    pub name: String,
    #[serde(default)]
    pub mode: VpnMode,
    #[serde(default)]
    pub provider: VpnProvider,
    #[serde(default)]
    pub config_path: Option<PathBuf>,
    #[serde(default)]
    pub interface: Option<String>,
    #[serde(default)]
    pub auto_connect: bool,
    #[serde(default = "default_true")]
    pub auto_disconnect: bool,
    #[serde(default)]
    pub probe_address: Option<String>,
    #[serde(default = "default_vpn_timeout")]
    pub connect_timeout_secs: u64,
}

impl VpnProfile {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: ServerConfigId::new(),
            name: name.into(),
            mode: VpnMode::default(),
            provider: VpnProvider::default(),
            config_path: None,
            interface: None,
            auto_connect: false,
            auto_disconnect: true,
            probe_address: None,
            connect_timeout_secs: default_vpn_timeout(),
        }
    }

    pub fn connect_timeout(&self) -> Duration {
        Duration::from_secs(self.connect_timeout_secs.max(1))
    }
}

fn default_ssh_port() -> u16 {
    22
}

fn default_connect_timeout() -> u64 {
    15
}

fn default_keepalive() -> u64 {
    30
}

fn default_vpn_timeout() -> u64 {
    30
}

fn default_bind_address() -> String {
    "127.0.0.1".to_owned()
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_ssh_config_uses_defaults() {
        let config: SshServerConfig = toml_like(
            r#"{"id":"6f1c7d2e-8a4b-4c3d-9e2f-1a2b3c4d5e6f","name":"Bastion","host":"bastion"}"#,
        );
        assert_eq!(config.port, 22);
        assert_eq!(config.auth, SshAuthMethod::PrivateKey);
        assert_eq!(config.host_key_policy, HostKeyPolicy::AskOnFirstUse);
        assert_eq!(config.local_bind_address, "127.0.0.1");
        assert!(config.reconnect.enabled);
        assert_eq!(config.keepalive_interval(), Some(Duration::from_secs(30)));
    }

    #[test]
    fn vpn_profiles_default_to_existing_connections() {
        let profile: VpnProfile =
            toml_like(r#"{"id":"6f1c7d2e-8a4b-4c3d-9e2f-1a2b3c4d5e6f","name":"Office"}"#);
        assert_eq!(profile.mode, VpnMode::Existing);
        assert!(profile.auto_disconnect);
        assert!(!profile.auto_connect);
    }

    #[test]
    fn endpoint_label_includes_user_when_present() {
        let mut config = SshServerConfig::new("Homelab", "192.168.178.10");
        assert_eq!(config.endpoint_label(), "192.168.178.10:22");
        config.username = "admin".into();
        assert_eq!(config.endpoint_label(), "admin@192.168.178.10:22");
    }

    fn toml_like<T: serde::de::DeserializeOwned>(json: &str) -> T {
        serde_json::from_str(json).unwrap()
    }
}
