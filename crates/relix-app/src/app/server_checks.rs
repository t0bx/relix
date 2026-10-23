/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use super::network_credentials::resolve_credentials;
use crate::events::AppEvent;
use crate::form::FormStatus;
use crate::notifications::Level;
use crate::overlay::{ConfirmState, Overlay, PendingAction, ViewerState};
use crate::server_manager::{ServerKind, ServerRow};
use relix_connections::{Approval, ConnectionError, NetworkPlan, VpnError};
use relix_core::{
    ConfigScope, NetworkMode, Secret, ServerConfigId, SshServerConfig, VpnMode, VpnProfile,
};
use relix_project::ServerConfig;

impl App {
    pub(crate) fn test_server(&mut self, row: &ServerRow) {
        match row.kind {
            ServerKind::Ssh => {
                let Some(config) = self.servers.ssh(row.id).cloned() else {
                    return;
                };
                if let Err(error) = self.start_ssh_test(config, None) {
                    self.notify(Level::Error, error);
                }
            }
            ServerKind::Vpn => {
                if let Some(profile) = self.servers.vpn(row.id).cloned() {
                    self.diagnose_vpn(profile);
                }
            }
        }
    }

    /// Tests an SSH configuration (as edited, not necessarily saved) by opening a session
    /// through its whole jump host chain, without forwarding anything.
    pub(crate) fn start_ssh_test(
        &mut self,
        config: SshServerConfig,
        secret: Option<Secret>,
    ) -> Result<(), String> {
        let scope = self
            .servers
            .scope_of(config.id)
            .unwrap_or(ConfigScope::Global);
        let mut candidate = self.servers.clone();
        candidate
            .registry_mut(scope)
            .upsert(ServerConfig::Ssh(config.clone()))
            .map_err(|error| error.to_string())?;
        let chain = candidate
            .jump_chain(config.id)
            .map_err(|error| error.to_string())?;
        let plan = NetworkPlan {
            mode: NetworkMode::SshTunnel,
            ssh_chain: chain,
            ..NetworkPlan::default()
        };
        let mut sources = self.credential_sources(&plan);
        if let Some(secret) = secret {
            let kind = match config.auth {
                relix_core::SshAuthMethod::Password => relix_project::ServerSecret::Password,
                _ => relix_project::ServerSecret::Passphrase,
            };
            self.server_secrets.insert((config.id, kind), secret);
            sources = self.credential_sources(&plan);
        }
        let secrets = self.context.secrets.clone();
        let network = self.context.network.clone();
        self.spawn(async move {
            let credentials = resolve_credentials(secrets, sources).await;
            let result = network.test_ssh(&plan.ssh_chain, &credentials.ssh).await;
            AppEvent::SshTested { plan, result }
        });
        Ok(())
    }

    pub(crate) fn on_ssh_tested(
        &mut self,
        plan: NetworkPlan,
        result: Result<String, ConnectionError>,
    ) {
        match result {
            Ok(endpoint) => self.update_ssh_form_status(FormStatus::success(format!(
                "✓ SSH session to {endpoint} established"
            ))),
            Err(error) => {
                self.handle_network_failure(None, Some(&plan), &error);
                self.update_ssh_form_status(FormStatus::failure(format!("✗ {error}")));
            }
        }
    }

    pub(crate) fn diagnose_vpn(&mut self, profile: VpnProfile) {
        let network = self.context.network.clone();
        let id = profile.id;
        self.spawn(async move {
            let lines = network.vpn().diagnose(&profile, None).await;
            AppEvent::VpnDiagnosed { profile: id, lines }
        });
    }

    pub(crate) fn on_vpn_diagnosed(&mut self, profile: ServerConfigId, lines: Vec<String>) {
        self.update_vpn_form_status(profile, FormStatus::success("diagnostics shown"));
        self.overlays.push(Overlay::Viewer(ViewerState::new(
            "VPN diagnostics",
            &lines.join("\n"),
        )));
    }

    pub(crate) fn toggle_vpn(&mut self, id: ServerConfigId) {
        let Some(profile) = self.servers.vpn(id) else {
            return;
        };
        match self.context.network.vpn_status(profile) {
            relix_connections::VpnStatus::Connected { owned: true, .. } => self.disconnect_vpn(id),
            relix_connections::VpnStatus::Connected { owned: false, .. } => self.notify(
                Level::Info,
                "this VPN is managed by the operating system; Relix does not disconnect it",
            ),
            _ => self.request_vpn_connect(id),
        }
    }

    pub(crate) fn request_vpn_connect(&mut self, id: ServerConfigId) {
        let Some(profile) = self.servers.vpn(id).cloned() else {
            return;
        };
        if profile.mode == VpnMode::Existing || self.vpn_approvals.contains(&id) {
            self.connect_vpn(profile, Approval::Granted);
            return;
        }
        match self.context.network.vpn().planned_command(&profile) {
            Ok(command) => self.overlays.push(Overlay::Confirm(ConfirmState {
                title: format!("Connect VPN '{}'?", profile.name),
                lines: vec![
                    "Relix will run this command with administrator rights:".into(),
                    format!("  {command}"),
                    String::new(),
                    "Your operating system asks for confirmation; Relix keeps running".into(),
                    "unprivileged. Disconnect it here or from the VPN dialog.".into(),
                ],
                confirm_label: "Run command".into(),
                danger: false,
                action: PendingAction::ConnectVpn(id),
            })),
            Err(error) => self.update_vpn_form_status(id, FormStatus::failure(error.to_string())),
        }
    }

    pub(crate) fn approve_and_connect_vpn(&mut self, id: ServerConfigId) {
        self.vpn_approvals.insert(id);
        if let Some(profile) = self.servers.vpn(id).cloned() {
            self.connect_vpn(profile, Approval::Granted);
        }
    }

    fn connect_vpn(&mut self, profile: VpnProfile, approval: Approval) {
        let network = self.context.network.clone();
        let id = profile.id;
        self.spawn(async move {
            let result = network
                .vpn()
                .connect(&profile, approval)
                .await
                .map(|()| format!("'{}' connected", profile.name));
            AppEvent::VpnUpdated {
                profile: id,
                result,
            }
        });
    }

    pub(crate) fn disconnect_vpn(&mut self, id: ServerConfigId) {
        let Some(profile) = self.servers.vpn(id).cloned() else {
            return;
        };
        let network = self.context.network.clone();
        self.spawn(async move {
            let result = network
                .vpn()
                .disconnect(&profile)
                .await
                .map(|()| format!("'{}' disconnected", profile.name));
            AppEvent::VpnUpdated {
                profile: id,
                result,
            }
        });
    }

    pub(crate) fn on_vpn_updated(
        &mut self,
        profile: ServerConfigId,
        result: Result<String, VpnError>,
    ) {
        let status = match result {
            Ok(message) => FormStatus::success(format!("✓ {message}")),
            Err(error) => FormStatus::failure(format!("✗ {error}")),
        };
        self.update_vpn_form_status(profile, status);
    }
}
