/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ConnectionError;
use relix_core::{DataSourceDefinition, NetworkMode, SshServerConfig, VpnProfile};
use relix_driver_api::ConnectionTarget;
use relix_project::ServerCatalog;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NetworkPlan {
    pub mode: NetworkMode,
    pub vpn: Option<VpnProfile>,
    pub ssh_chain: Vec<SshServerConfig>,
    pub local_port: Option<u16>,
}

impl NetworkPlan {
    pub fn direct() -> Self {
        Self::default()
    }

    pub fn ssh_server(&self) -> Option<&SshServerConfig> {
        self.ssh_chain.last()
    }

    pub fn describe(&self) -> String {
        match (&self.vpn, self.ssh_server()) {
            (Some(vpn), Some(ssh)) => format!("via VPN {} + SSH {}", vpn.name, ssh.name),
            (Some(vpn), None) => format!("via VPN {}", vpn.name),
            (None, Some(ssh)) => format!("via SSH {}", ssh.name),
            (None, None) => "directly".into(),
        }
    }
}

/// Resolves a data source's network settings against the server catalog. SSH local
/// forwarding needs a single TCP endpoint, so it is refused for file databases and for
/// cloud APIs that talk to several provider endpoints; a VPN works for anything that
/// uses the network because it is applied by the operating system.
pub fn plan_network(
    definition: &DataSourceDefinition,
    catalog: &ServerCatalog,
    target: ConnectionTarget,
    engine: &str,
) -> Result<NetworkPlan, ConnectionError> {
    let network = &definition.network;
    if network.is_direct() {
        return Ok(NetworkPlan::direct());
    }
    if target == ConnectionTarget::File {
        return Err(ConnectionError::UnsupportedTransport(format!(
            "{engine} opens a local file; SSH tunnels and VPN profiles do not apply"
        )));
    }
    if network.mode.uses_ssh() && !target.supports_tunnel() {
        return Err(ConnectionError::UnsupportedTransport(format!(
            "{engine} connects to cloud provider APIs over several HTTPS endpoints, which an SSH \
             port forward cannot carry; connect directly, through a VPN or a system proxy"
        )));
    }
    catalog.validate_data_source(definition)?;
    let vpn = match network.vpn_profile() {
        Some(id) => Some(catalog.vpn_reference(id)?.clone()),
        None => None,
    };
    let ssh_chain = match network.ssh_config() {
        Some(id) => catalog.jump_chain(id)?,
        None => Vec::new(),
    };
    Ok(NetworkPlan {
        mode: network.mode,
        vpn,
        ssh_chain,
        local_port: network.local_port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::{ConfigScope, DriverId, NetworkSettings, ProjectId};
    use relix_project::{ServerConfig, ServerRegistry};

    fn catalog() -> (ServerCatalog, SshServerConfig, VpnProfile) {
        let ssh = SshServerConfig::new("Production Bastion", "bastion.example.com");
        let vpn = VpnProfile::new("Corporate WireGuard");
        let mut catalog = ServerCatalog::new(
            ServerRegistry::default(),
            ServerRegistry::default(),
            Some(ProjectId::new()),
        );
        catalog
            .registry_mut(ConfigScope::Global)
            .upsert(ServerConfig::Ssh(ssh.clone()))
            .unwrap();
        catalog
            .registry_mut(ConfigScope::Project)
            .upsert(ServerConfig::Vpn(vpn.clone()))
            .unwrap();
        (catalog, ssh, vpn)
    }

    fn source(mode: NetworkMode, ssh: &SshServerConfig, vpn: &VpnProfile) -> DataSourceDefinition {
        let mut definition = DataSourceDefinition::new("Production", DriverId::from("postgres"));
        definition.network = NetworkSettings {
            mode,
            ssh: Some(ssh.id),
            vpn: Some(vpn.id),
            local_port: Some(15432),
        };
        definition
    }

    #[test]
    fn vpn_and_ssh_are_combined() {
        let (catalog, ssh, vpn) = catalog();
        let network = ConnectionTarget::Network { default_port: 5432 };
        let plan = plan_network(
            &source(NetworkMode::VpnSshTunnel, &ssh, &vpn),
            &catalog,
            network,
            "PostgreSQL",
        )
        .unwrap();
        assert_eq!(
            plan.vpn.as_ref().map(|vpn| vpn.name.as_str()),
            Some("Corporate WireGuard")
        );
        assert_eq!(plan.ssh_chain.len(), 1);
        assert_eq!(plan.local_port, Some(15432));
        assert_eq!(
            plan.describe(),
            "via VPN Corporate WireGuard + SSH Production Bastion"
        );
        let direct = plan_network(
            &source(NetworkMode::Direct, &ssh, &vpn),
            &catalog,
            network,
            "PostgreSQL",
        )
        .unwrap();
        assert_eq!(direct.describe(), "directly");
    }

    #[test]
    fn cloud_engines_refuse_ssh_but_accept_vpn() {
        let (catalog, ssh, vpn) = catalog();
        let error = plan_network(
            &source(NetworkMode::SshTunnel, &ssh, &vpn),
            &catalog,
            ConnectionTarget::Cloud,
            "BigQuery",
        )
        .unwrap_err();
        assert!(matches!(error, ConnectionError::UnsupportedTransport(_)));
        let plan = plan_network(
            &source(NetworkMode::Vpn, &ssh, &vpn),
            &catalog,
            ConnectionTarget::Cloud,
            "BigQuery",
        )
        .unwrap();
        assert!(plan.vpn.is_some() && plan.ssh_chain.is_empty());
    }

    #[test]
    fn deleted_configurations_are_reported() {
        let (catalog, ssh, vpn) = catalog();
        let mut definition = source(NetworkMode::SshTunnel, &ssh, &vpn);
        definition.network.ssh = Some(relix_core::ServerConfigId::new());
        let error = plan_network(
            &definition,
            &catalog,
            ConnectionTarget::Network { default_port: 5432 },
            "PostgreSQL",
        )
        .unwrap_err();
        assert!(error.to_string().contains("no longer exists"));
    }
}
