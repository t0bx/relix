/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::SshError;
use crate::openssh_config::{self, OpenSshHost, expand_home};
use relix_core::{HostKeyPolicy, Secret, SshAuthMethod, SshServerConfig};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHop {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: SshAuthMethod,
    pub private_key: Option<PathBuf>,
    pub host_key_policy: HostKeyPolicy,
    pub known_hosts_file: Option<PathBuf>,
    pub connect_timeout: Duration,
    pub keepalive: Option<Duration>,
}

impl ResolvedHop {
    pub fn endpoint(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    pub fn from_config(config: &SshServerConfig) -> Result<Self, SshError> {
        let openssh = if config.use_ssh_config {
            openssh_config::user_config_path()
                .and_then(|path| openssh_config::lookup(&path, &config.host))
                .unwrap_or_default()
        } else {
            OpenSshHost::default()
        };
        Self::merge(config, openssh)
    }

    pub(crate) fn merge(config: &SshServerConfig, openssh: OpenSshHost) -> Result<Self, SshError> {
        let host = openssh
            .host_name
            .unwrap_or_else(|| config.host.trim().to_owned());
        if host.is_empty() {
            return Err(SshError::Configuration(format!(
                "'{}' has no SSH host",
                config.name
            )));
        }
        let user = Some(config.username.trim())
            .filter(|user| !user.is_empty())
            .map(str::to_owned)
            .or(openssh.user)
            .ok_or_else(|| {
                SshError::Configuration(format!("'{}' has no SSH user name", config.name))
            })?;
        let port = if config.use_ssh_config && config.port == 22 {
            openssh.port.unwrap_or(config.port)
        } else {
            config.port
        };
        let private_key = config
            .private_key
            .as_deref()
            .map(expand_home)
            .or(openssh.identity_file);
        if config.auth == SshAuthMethod::PrivateKey && private_key.is_none() {
            return Err(SshError::Configuration(format!(
                "'{}' uses key authentication but no private key file is set",
                config.name
            )));
        }
        Ok(Self {
            name: config.name.clone(),
            host,
            port,
            user,
            auth: config.auth,
            private_key,
            host_key_policy: config.host_key_policy,
            known_hosts_file: config.known_hosts_file.as_deref().map(expand_home),
            connect_timeout: config.connect_timeout(),
            keepalive: config.keepalive_interval(),
        })
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct HopSecrets {
    pub password: Option<Secret>,
    pub passphrase: Option<Secret>,
}

impl std::fmt::Debug for HopSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HopSecrets")
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("passphrase", &self.passphrase.as_ref().map(|_| "***"))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_settings_override_the_openssh_config() {
        let mut config = SshServerConfig::new("Bastion", "bastion");
        config.username = "deploy".into();
        config.use_ssh_config = true;
        config.auth = SshAuthMethod::Agent;
        let openssh = OpenSshHost {
            host_name: Some("bastion.example.com".into()),
            port: Some(2222),
            user: Some("other".into()),
            identity_file: None,
        };
        let hop = ResolvedHop::merge(&config, openssh).unwrap();
        assert_eq!(hop.host, "bastion.example.com");
        assert_eq!(hop.port, 2222);
        assert_eq!(hop.user, "deploy");
    }

    #[test]
    fn key_authentication_requires_a_key_file() {
        let mut config = SshServerConfig::new("Bastion", "bastion");
        config.username = "deploy".into();
        let error = ResolvedHop::merge(&config, OpenSshHost::default()).unwrap_err();
        assert!(matches!(error, SshError::Configuration(_)));
        config.private_key = Some("/keys/id_ed25519".into());
        assert!(ResolvedHop::merge(&config, OpenSshHost::default()).is_ok());
    }

    #[test]
    fn secrets_are_redacted() {
        let secrets = HopSecrets {
            password: Some(Secret::new("hunter2")),
            passphrase: None,
        };
        assert!(!format!("{secrets:?}").contains("hunter2"));
    }
}
