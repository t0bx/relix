/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ConnectionError;
use crate::leased::{LeasedConnection, NetworkLeases};
use crate::plan::NetworkPlan;
use relix_core::ConnectionConfig;
use relix_core::VpnProfile;
use relix_driver_api::{DatabaseConnection, DatabaseDriver, DriverError, ServerInfo};
use relix_ssh::{
    HopSecrets, HostKeyPrompt, SessionSummary, SshError, TunnelManager, TunnelRequest,
};
use relix_vpn::{Approval, VpnManager, VpnStatus};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct NetworkCredentials {
    pub ssh: Vec<HopSecrets>,
    pub vpn_approval: Option<Approval>,
}

#[derive(Clone)]
pub struct ConnectionManager {
    tunnels: TunnelManager,
    vpn: VpnManager,
}

impl ConnectionManager {
    pub fn new(tunnels: TunnelManager, vpn: VpnManager) -> Self {
        Self { tunnels, vpn }
    }

    pub fn tunnels(&self) -> &TunnelManager {
        &self.tunnels
    }

    pub fn vpn(&self) -> &VpnManager {
        &self.vpn
    }

    /// Establishes the network route and then the database session: the VPN first (so the
    /// SSH server becomes reachable), then the SSH tunnel, then the driver against the
    /// tunnel's local endpoint while TLS keeps verifying the original server name. Each
    /// stage holds a lease that the returned connection releases when it is closed.
    pub async fn connect(
        &self,
        driver: &dyn DatabaseDriver,
        config: &ConnectionConfig,
        plan: &NetworkPlan,
        credentials: &NetworkCredentials,
    ) -> Result<Arc<dyn DatabaseConnection>, ConnectionError> {
        let (config, leases) = self.open_route(driver, config, plan, credentials).await?;
        let connection = driver.connect(&config).await?;
        if leases.is_empty() {
            return Ok(Arc::from(connection));
        }
        Ok(Arc::new(LeasedConnection::new(
            connection,
            leases,
            plan.describe(),
        )))
    }

    pub async fn test(
        &self,
        driver: &dyn DatabaseDriver,
        config: &ConnectionConfig,
        plan: &NetworkPlan,
        credentials: &NetworkCredentials,
    ) -> Result<(ServerInfo, String), ConnectionError> {
        let connection = self.connect(driver, config, plan, credentials).await?;
        let checked: Result<ServerInfo, DriverError> = async {
            connection.ping().await?;
            Ok(connection.server_info())
        }
        .await;
        let _ = connection.close().await;
        Ok((checked?, plan.describe()))
    }

    async fn open_route(
        &self,
        driver: &dyn DatabaseDriver,
        config: &ConnectionConfig,
        plan: &NetworkPlan,
        credentials: &NetworkCredentials,
    ) -> Result<(ConnectionConfig, NetworkLeases), ConnectionError> {
        let mut leases = NetworkLeases::default();
        if let Some(profile) = &plan.vpn {
            let approval = credentials.vpn_approval.unwrap_or(Approval::NotGranted);
            leases.vpn = Some(self.vpn.acquire(profile, approval).await?);
        }
        if plan.ssh_chain.is_empty() {
            return Ok((config.clone(), leases));
        }
        let target_port = config
            .settings
            .port
            .or_else(|| driver.metadata().default_port())
            .ok_or_else(|| {
                ConnectionError::UnsupportedTransport("the engine has no network port".into())
            })?;
        let lease = self
            .tunnels
            .open(TunnelRequest {
                chain: plan.ssh_chain.clone(),
                secrets: credentials.ssh.clone(),
                target_host: config.host_or_default().to_owned(),
                target_port,
                local_port: plan.local_port,
            })
            .await?;
        let local = lease.local_address();
        let redirected = config.redirected(&local.ip().to_string(), local.port());
        leases.tunnel = Some(lease);
        Ok((redirected, leases))
    }

    pub fn trust_host_key(&self, prompt: &HostKeyPrompt) -> Result<(), SshError> {
        self.tunnels.trust_host_key(prompt)
    }

    pub fn ssh_sessions(&self) -> Vec<SessionSummary> {
        self.tunnels.sessions()
    }

    pub fn vpn_status(&self, profile: &VpnProfile) -> VpnStatus {
        self.vpn.status(profile)
    }
}
