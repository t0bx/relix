/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ProjectError;
use crate::model::validate_name;
use relix_core::{ServerConfigId, SshServerConfig, VpnProfile};
use serde::{Deserialize, Serialize};

pub const SERVERS_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerConfig {
    Ssh(SshServerConfig),
    Vpn(VpnProfile),
}

impl ServerConfig {
    pub fn id(&self) -> ServerConfigId {
        match self {
            Self::Ssh(config) => config.id,
            Self::Vpn(profile) => profile.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Ssh(config) => &config.name,
            Self::Vpn(profile) => &profile.name,
        }
    }

    fn set_identity(&mut self, id: ServerConfigId, name: String) {
        match self {
            Self::Ssh(config) => {
                config.id = id;
                config.name = name;
            }
            Self::Vpn(profile) => {
                profile.id = id;
                profile.name = name;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerRegistry {
    #[serde(default = "current_version")]
    pub format_version: u32,
    #[serde(default)]
    pub ssh: Vec<SshServerConfig>,
    #[serde(default)]
    pub vpn: Vec<VpnProfile>,
}

impl Default for ServerRegistry {
    fn default() -> Self {
        Self {
            format_version: SERVERS_FORMAT_VERSION,
            ssh: Vec::new(),
            vpn: Vec::new(),
        }
    }
}

impl ServerRegistry {
    pub fn is_empty(&self) -> bool {
        self.ssh.is_empty() && self.vpn.is_empty()
    }

    pub fn ssh(&self, id: ServerConfigId) -> Option<&SshServerConfig> {
        self.ssh.iter().find(|config| config.id == id)
    }

    pub fn vpn(&self, id: ServerConfigId) -> Option<&VpnProfile> {
        self.vpn.iter().find(|profile| profile.id == id)
    }

    pub fn get(&self, id: ServerConfigId) -> Option<ServerConfig> {
        self.ssh(id)
            .cloned()
            .map(ServerConfig::Ssh)
            .or_else(|| self.vpn(id).cloned().map(ServerConfig::Vpn))
    }

    pub fn contains(&self, id: ServerConfigId) -> bool {
        self.ssh(id).is_some() || self.vpn(id).is_some()
    }

    /// Inserts or replaces a configuration. Names are trimmed and must be unique per kind
    /// within the registry, ignoring case, so references shown in the UI are unambiguous.
    pub fn upsert(&mut self, config: ServerConfig) -> Result<(), ProjectError> {
        let name = validate_name(config.name())?;
        let id = config.id();
        let clash = match &config {
            ServerConfig::Ssh(_) => self
                .ssh
                .iter()
                .any(|other| other.id != id && other.name.eq_ignore_ascii_case(&name)),
            ServerConfig::Vpn(_) => self
                .vpn
                .iter()
                .any(|other| other.id != id && other.name.eq_ignore_ascii_case(&name)),
        };
        if clash {
            return Err(ProjectError::DuplicateName(name));
        }
        let mut config = config;
        config.set_identity(id, name);
        match config {
            ServerConfig::Ssh(config) => upsert_by_id(&mut self.ssh, config, |entry| entry.id),
            ServerConfig::Vpn(profile) => upsert_by_id(&mut self.vpn, profile, |entry| entry.id),
        }
        Ok(())
    }

    pub fn remove(&mut self, id: ServerConfigId) -> Option<ServerConfig> {
        if let Some(index) = self.ssh.iter().position(|config| config.id == id) {
            return Some(ServerConfig::Ssh(self.ssh.remove(index)));
        }
        let index = self.vpn.iter().position(|profile| profile.id == id)?;
        Some(ServerConfig::Vpn(self.vpn.remove(index)))
    }

    pub fn rename(&mut self, id: ServerConfigId, name: &str) -> Result<(), ProjectError> {
        let mut config = self
            .get(id)
            .ok_or_else(|| ProjectError::InvalidName("configuration not found".into()))?;
        config.set_identity(id, name.to_owned());
        self.upsert(config)
    }

    pub fn duplicate(&mut self, id: ServerConfigId) -> Option<ServerConfigId> {
        let mut copy = self.get(id)?;
        let name = self.unique_name(&copy, &format!("{} copy", copy.name()));
        let new_id = ServerConfigId::new();
        copy.set_identity(new_id, name);
        self.upsert(copy).ok()?;
        Some(new_id)
    }

    /// Adds a configuration coming from another scope, renaming it when its name is taken
    /// here. The caller decides whether the identifier is kept (move) or replaced (copy).
    pub fn adopt(&mut self, mut config: ServerConfig, id: ServerConfigId) -> ServerConfigId {
        let name = self.unique_name(&config, config.name());
        config.set_identity(id, name);
        match config {
            ServerConfig::Ssh(config) => upsert_by_id(&mut self.ssh, config, |entry| entry.id),
            ServerConfig::Vpn(profile) => upsert_by_id(&mut self.vpn, profile, |entry| entry.id),
        }
        id
    }

    fn unique_name(&self, config: &ServerConfig, base: &str) -> String {
        let taken = |candidate: &str| match config {
            ServerConfig::Ssh(_) => self
                .ssh
                .iter()
                .any(|other| other.name.eq_ignore_ascii_case(candidate)),
            ServerConfig::Vpn(_) => self
                .vpn
                .iter()
                .any(|other| other.name.eq_ignore_ascii_case(candidate)),
        };
        let mut candidate = base.to_owned();
        let mut counter = 2;
        while taken(&candidate) {
            candidate = format!("{base} {counter}");
            counter += 1;
        }
        candidate
    }
}

fn upsert_by_id<T>(entries: &mut Vec<T>, entry: T, id: impl Fn(&T) -> ServerConfigId) {
    let key = id(&entry);
    match entries.iter_mut().find(|existing| id(existing) == key) {
        Some(existing) => *existing = entry,
        None => entries.push(entry),
    }
}

fn current_version() -> u32 {
    SERVERS_FORMAT_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bastion(name: &str) -> ServerConfig {
        ServerConfig::Ssh(SshServerConfig::new(name, "bastion.example.com"))
    }

    #[test]
    fn names_are_unique_per_kind() {
        let mut registry = ServerRegistry::default();
        registry.upsert(bastion("Production Bastion")).unwrap();
        assert!(matches!(
            registry.upsert(bastion("production bastion")),
            Err(ProjectError::DuplicateName(_))
        ));
        registry
            .upsert(ServerConfig::Vpn(VpnProfile::new("Production Bastion")))
            .unwrap();
        assert_eq!(registry.ssh.len(), 1);
        assert_eq!(registry.vpn.len(), 1);
    }

    #[test]
    fn rename_keeps_the_identifier() {
        let mut registry = ServerRegistry::default();
        let config = bastion("Homelab");
        let id = config.id();
        registry.upsert(config).unwrap();
        registry.rename(id, "  Homelab SSH ").unwrap();
        assert_eq!(registry.ssh(id).unwrap().name, "Homelab SSH");
        assert!(registry.rename(ServerConfigId::new(), "x").is_err());
    }

    #[test]
    fn duplicates_get_fresh_ids_and_unique_names() {
        let mut registry = ServerRegistry::default();
        let config = bastion("Gateway");
        let id = config.id();
        registry.upsert(config).unwrap();
        let first = registry.duplicate(id).unwrap();
        let second = registry.duplicate(id).unwrap();
        assert_ne!(first, id);
        assert_eq!(registry.ssh(first).unwrap().name, "Gateway copy");
        assert_eq!(registry.ssh(second).unwrap().name, "Gateway copy 2");
    }

    #[test]
    fn adopted_configs_are_renamed_on_clash() {
        let mut registry = ServerRegistry::default();
        registry.upsert(bastion("Gateway")).unwrap();
        let incoming = bastion("Gateway");
        let id = incoming.id();
        registry.adopt(incoming, id);
        assert_eq!(registry.ssh(id).unwrap().name, "Gateway 2");
        assert!(matches!(registry.remove(id), Some(ServerConfig::Ssh(_))));
        assert!(!registry.contains(id));
    }
}
