/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::servers::{ServerConfig, ServerRegistry};
use relix_core::{
    ConfigScope, DataSourceDefinition, ProjectId, ServerConfigId, SshServerConfig, VpnProfile,
};

const MAX_JUMP_HOPS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReferenceError {
    #[error("the referenced {kind} configuration no longer exists")]
    Missing { kind: &'static str },
    #[error("'{name}' is not an {expected} configuration")]
    WrongKind {
        name: String,
        expected: &'static str,
    },
    #[error("jump hosts form a loop: {0}")]
    Cycle(String),
    #[error("jump host chain is longer than {MAX_JUMP_HOPS} hops")]
    TooDeep,
    #[error("global configuration '{0}' cannot use a project-specific jump host")]
    ScopeLeak(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferMode {
    Move,
    Copy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub scope: ConfigScope,
    pub config: ServerConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerCatalog {
    global: ServerRegistry,
    project: ServerRegistry,
    project_id: Option<ProjectId>,
    global_read_only: bool,
    project_read_only: bool,
}

impl ServerCatalog {
    pub fn new(
        global: ServerRegistry,
        project: ServerRegistry,
        project_id: Option<ProjectId>,
    ) -> Self {
        Self {
            global,
            project,
            project_id,
            global_read_only: false,
            project_read_only: project_id.is_none(),
        }
    }

    pub fn project_id(&self) -> Option<ProjectId> {
        self.project_id
    }

    pub fn registry(&self, scope: ConfigScope) -> &ServerRegistry {
        match scope {
            ConfigScope::Global => &self.global,
            ConfigScope::Project => &self.project,
        }
    }

    pub fn registry_mut(&mut self, scope: ConfigScope) -> &mut ServerRegistry {
        match scope {
            ConfigScope::Global => &mut self.global,
            ConfigScope::Project => &mut self.project,
        }
    }

    pub fn is_read_only(&self, scope: ConfigScope) -> bool {
        match scope {
            ConfigScope::Global => self.global_read_only,
            ConfigScope::Project => self.project_read_only,
        }
    }

    pub fn set_read_only(&mut self, scope: ConfigScope, read_only: bool) {
        match scope {
            ConfigScope::Global => self.global_read_only = read_only,
            ConfigScope::Project => self.project_read_only = read_only,
        }
    }

    pub fn find(&self, id: ServerConfigId) -> Option<CatalogEntry> {
        [ConfigScope::Project, ConfigScope::Global]
            .into_iter()
            .find_map(|scope| {
                self.registry(scope)
                    .get(id)
                    .map(|config| CatalogEntry { scope, config })
            })
    }

    pub fn ssh(&self, id: ServerConfigId) -> Option<&SshServerConfig> {
        self.project.ssh(id).or_else(|| self.global.ssh(id))
    }

    pub fn vpn(&self, id: ServerConfigId) -> Option<&VpnProfile> {
        self.project.vpn(id).or_else(|| self.global.vpn(id))
    }

    pub fn scope_of(&self, id: ServerConfigId) -> Option<ConfigScope> {
        self.find(id).map(|entry| entry.scope)
    }

    pub fn entries(&self) -> Vec<CatalogEntry> {
        let mut entries = Vec::new();
        for ssh in [true, false] {
            for scope in [ConfigScope::Global, ConfigScope::Project] {
                let registry = self.registry(scope);
                let configs: Vec<ServerConfig> = if ssh {
                    registry
                        .ssh
                        .iter()
                        .cloned()
                        .map(ServerConfig::Ssh)
                        .collect()
                } else {
                    registry
                        .vpn
                        .iter()
                        .cloned()
                        .map(ServerConfig::Vpn)
                        .collect()
                };
                let mut configs = configs;
                configs.sort_by_key(|config| config.name().to_lowercase());
                entries.extend(
                    configs
                        .into_iter()
                        .map(|config| CatalogEntry { scope, config }),
                );
            }
        }
        entries
    }

    /// Resolves the SSH hops needed to reach `target`: the first element is the server
    /// Relix connects to directly, the last one is `target` itself. Missing references,
    /// loops and global configurations that depend on project-specific hops are rejected.
    pub fn jump_chain(
        &self,
        target: ServerConfigId,
    ) -> Result<Vec<SshServerConfig>, ReferenceError> {
        let mut chain: Vec<SshServerConfig> = Vec::new();
        let mut next = Some(target);
        while let Some(id) = next {
            if chain.iter().any(|hop| hop.id == id) {
                let mut names: Vec<&str> =
                    chain.iter().rev().map(|hop| hop.name.as_str()).collect();
                names.push(self.ssh(id).map_or("?", |hop| hop.name.as_str()));
                return Err(ReferenceError::Cycle(names.join(" → ")));
            }
            if chain.len() == MAX_JUMP_HOPS {
                return Err(ReferenceError::TooDeep);
            }
            let hop = self.ssh_reference(id)?;
            if let Some(previous) = chain.last()
                && self.scope_of(previous.id) == Some(ConfigScope::Global)
                && self.scope_of(id) == Some(ConfigScope::Project)
            {
                return Err(ReferenceError::ScopeLeak(previous.name.clone()));
            }
            next = hop.jump_host;
            chain.push(hop.clone());
        }
        chain.reverse();
        Ok(chain)
    }

    pub fn ssh_reference(&self, id: ServerConfigId) -> Result<&SshServerConfig, ReferenceError> {
        if let Some(config) = self.ssh(id) {
            return Ok(config);
        }
        match self.vpn(id) {
            Some(profile) => Err(ReferenceError::WrongKind {
                name: profile.name.clone(),
                expected: "SSH",
            }),
            None => Err(ReferenceError::Missing { kind: "SSH" }),
        }
    }

    pub fn vpn_reference(&self, id: ServerConfigId) -> Result<&VpnProfile, ReferenceError> {
        if let Some(profile) = self.vpn(id) {
            return Ok(profile);
        }
        match self.ssh(id) {
            Some(config) => Err(ReferenceError::WrongKind {
                name: config.name.clone(),
                expected: "VPN",
            }),
            None => Err(ReferenceError::Missing { kind: "VPN" }),
        }
    }

    pub fn validate_data_source(
        &self,
        definition: &DataSourceDefinition,
    ) -> Result<(), ReferenceError> {
        let network = &definition.network;
        if network.mode.uses_ssh() {
            let id = network.ssh.ok_or(ReferenceError::Missing { kind: "SSH" })?;
            self.jump_chain(id)?;
        }
        if network.mode.uses_vpn() {
            let id = network.vpn.ok_or(ReferenceError::Missing { kind: "VPN" })?;
            self.vpn_reference(id)?;
        }
        Ok(())
    }

    pub fn usages<'a>(
        &self,
        id: ServerConfigId,
        sources: &'a [DataSourceDefinition],
    ) -> Vec<&'a DataSourceDefinition> {
        sources
            .iter()
            .filter(|source| source.network.references(id))
            .collect()
    }

    pub fn dependent_ssh_configs(&self, id: ServerConfigId) -> Vec<&SshServerConfig> {
        self.global
            .ssh
            .iter()
            .chain(&self.project.ssh)
            .filter(|config| config.jump_host == Some(id))
            .collect()
    }

    /// Moves or copies a configuration into the other scope. A move keeps the identifier,
    /// so every data source keeps resolving it; a copy gets a fresh identifier and leaves
    /// the original untouched. Returns the identifier in the target scope.
    pub fn transfer(
        &mut self,
        id: ServerConfigId,
        to: ConfigScope,
        mode: TransferMode,
    ) -> Option<ServerConfigId> {
        let entry = self.find(id)?;
        if entry.scope == to {
            return None;
        }
        let target_id = match mode {
            TransferMode::Move => {
                self.registry_mut(entry.scope).remove(id);
                id
            }
            TransferMode::Copy => ServerConfigId::new(),
        };
        Some(self.registry_mut(to).adopt(entry.config, target_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::{NetworkMode, NetworkSettings};

    fn ssh(name: &str, jump: Option<ServerConfigId>) -> SshServerConfig {
        let mut config = SshServerConfig::new(name, format!("{}.example.com", name.to_lowercase()));
        config.jump_host = jump;
        config
    }

    fn catalog_with(global: Vec<SshServerConfig>, project: Vec<SshServerConfig>) -> ServerCatalog {
        let mut catalog = ServerCatalog::new(
            ServerRegistry::default(),
            ServerRegistry::default(),
            Some(ProjectId::new()),
        );
        for config in global {
            catalog
                .registry_mut(ConfigScope::Global)
                .upsert(ServerConfig::Ssh(config))
                .unwrap();
        }
        for config in project {
            catalog
                .registry_mut(ConfigScope::Project)
                .upsert(ServerConfig::Ssh(config))
                .unwrap();
        }
        catalog
    }

    #[test]
    fn jump_chain_orders_hops_from_the_entry_point() {
        let gateway_a = ssh("GatewayA", None);
        let gateway_b = ssh("GatewayB", Some(gateway_a.id));
        let target = gateway_b.id;
        let catalog = catalog_with(vec![gateway_a, gateway_b], vec![]);
        let names: Vec<String> = catalog
            .jump_chain(target)
            .unwrap()
            .into_iter()
            .map(|hop| hop.name)
            .collect();
        assert_eq!(names, vec!["GatewayA", "GatewayB"]);
    }

    #[test]
    fn loops_and_missing_hops_are_reported() {
        let mut first = ssh("First", None);
        let second = ssh("Second", Some(first.id));
        first.jump_host = Some(second.id);
        let catalog = catalog_with(vec![first.clone(), second], vec![]);
        assert!(matches!(
            catalog.jump_chain(first.id),
            Err(ReferenceError::Cycle(_))
        ));
        let dangling = ssh("Dangling", Some(ServerConfigId::new()));
        let catalog = catalog_with(vec![dangling.clone()], vec![]);
        assert_eq!(
            catalog.jump_chain(dangling.id),
            Err(ReferenceError::Missing { kind: "SSH" })
        );
    }

    #[test]
    fn global_configs_cannot_depend_on_project_hops() {
        let local = ssh("Local", None);
        let global = ssh("Global", Some(local.id));
        let catalog = catalog_with(vec![global.clone()], vec![local]);
        assert_eq!(
            catalog.jump_chain(global.id),
            Err(ReferenceError::ScopeLeak("Global".into()))
        );
    }

    #[test]
    fn data_source_references_are_validated_by_kind() {
        let bastion = ssh("Bastion", None);
        let mut catalog = catalog_with(vec![bastion.clone()], vec![]);
        let vpn = VpnProfile::new("Office");
        catalog
            .registry_mut(ConfigScope::Project)
            .upsert(ServerConfig::Vpn(vpn.clone()))
            .unwrap();
        let mut source = DataSourceDefinition::new("Production", "postgres".into());
        source.network = NetworkSettings {
            mode: NetworkMode::VpnSshTunnel,
            ssh: Some(bastion.id),
            vpn: Some(vpn.id),
            local_port: None,
        };
        assert_eq!(catalog.validate_data_source(&source), Ok(()));
        source.network.vpn = Some(bastion.id);
        assert!(matches!(
            catalog.validate_data_source(&source),
            Err(ReferenceError::WrongKind { .. })
        ));
        let sources = vec![source];
        assert_eq!(catalog.usages(bastion.id, &sources).len(), 1);
    }

    #[test]
    fn move_keeps_identity_and_copy_creates_a_new_one() {
        let bastion = ssh("Bastion", None);
        let id = bastion.id;
        let mut catalog = catalog_with(vec![], vec![bastion]);
        let copied = catalog
            .transfer(id, ConfigScope::Global, TransferMode::Copy)
            .unwrap();
        assert_ne!(copied, id);
        assert_eq!(catalog.scope_of(id), Some(ConfigScope::Project));
        assert_eq!(catalog.scope_of(copied), Some(ConfigScope::Global));
        let mut catalog = catalog_with(vec![], vec![ssh("Moved", None)]);
        let moved_id = catalog.registry(ConfigScope::Project).ssh[0].id;
        let moved = catalog
            .transfer(moved_id, ConfigScope::Global, TransferMode::Move)
            .unwrap();
        assert_eq!(moved, moved_id);
        assert_eq!(catalog.scope_of(moved_id), Some(ConfigScope::Global));
        assert!(catalog.registry(ConfigScope::Project).is_empty());
    }

    #[test]
    fn entries_list_ssh_before_vpn_and_global_before_project() {
        let mut catalog = catalog_with(vec![ssh("Zeta", None)], vec![ssh("Alpha", None)]);
        catalog
            .registry_mut(ConfigScope::Global)
            .upsert(ServerConfig::Vpn(VpnProfile::new("Office")))
            .unwrap();
        let labels: Vec<(ConfigScope, String)> = catalog
            .entries()
            .into_iter()
            .map(|entry| (entry.scope, entry.config.name().to_owned()))
            .collect();
        assert_eq!(
            labels,
            vec![
                (ConfigScope::Global, "Zeta".into()),
                (ConfigScope::Project, "Alpha".into()),
                (ConfigScope::Global, "Office".into()),
            ]
        );
    }
}
