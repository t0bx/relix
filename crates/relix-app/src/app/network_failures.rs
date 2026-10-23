/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use crate::input::LineInput;
use crate::notifications::Level;
use crate::overlay::{ConfirmState, Overlay, PendingAction, PromptAction, PromptState};
use relix_connections::{
    ConnectionError, CredentialKind, HostKeyPrompt, NetworkPlan, SshError, VpnError,
};
use relix_core::{DataSourceId, Secret, ServerConfigId};
use relix_project::ServerSecret;

impl App {
    /// Reacts to failures of the network route that the user can resolve: an unknown
    /// SSH host key (explicit trust dialog), missing or rejected SSH credentials (masked
    /// prompt) and VPN commands that need administrator approval. Returns `true` when an
    /// interactive follow-up was opened; the connection is retried once it is answered.
    pub(crate) fn handle_network_failure(
        &mut self,
        source: Option<DataSourceId>,
        plan: Option<&NetworkPlan>,
        error: &ConnectionError,
    ) -> bool {
        match error {
            ConnectionError::Ssh(SshError::HostKeyUnknown(prompt)) => {
                self.ask_to_trust_host_key(source, prompt);
                true
            }
            ConnectionError::Ssh(SshError::CredentialsRequired { server, kind }) => {
                let Some(hop) =
                    plan.and_then(|plan| plan.ssh_chain.iter().find(|hop| &hop.name == server))
                else {
                    return false;
                };
                self.prompt_server_secret(source, hop.id, server, *kind);
                true
            }
            ConnectionError::Ssh(SshError::Authentication { endpoint, .. }) => {
                let Some(hop) = plan.and_then(|plan| {
                    plan.ssh_chain
                        .iter()
                        .find(|hop| format!("{}:{}", hop.host, hop.port) == *endpoint)
                }) else {
                    return false;
                };
                let kind = match hop.auth {
                    relix_core::SshAuthMethod::Password => CredentialKind::Password,
                    _ => return false,
                };
                self.server_secrets
                    .remove(&(hop.id, ServerSecret::Password));
                let (id, name) = (hop.id, hop.name.clone());
                self.prompt_server_secret(source, id, &name, kind);
                true
            }
            ConnectionError::Vpn(VpnError::ApprovalRequired { profile, command }) => {
                let Some(vpn) = plan.and_then(|plan| plan.vpn.as_ref()) else {
                    return false;
                };
                self.overlays.push(Overlay::Confirm(ConfirmState {
                    title: format!("Connect VPN '{profile}'?"),
                    lines: vec![
                        "Relix will run this command with administrator rights:".into(),
                        format!("  {command}"),
                        String::new(),
                        "Your operating system asks for confirmation; Relix itself keeps".into(),
                        "running unprivileged. The VPN is disconnected again when the last".into(),
                        "connection using it closes (if auto-disconnect is enabled).".into(),
                    ],
                    confirm_label: "Run command".into(),
                    danger: false,
                    action: PendingAction::ApproveVpn {
                        profile: vpn.id,
                        retry: source,
                    },
                }));
                true
            }
            _ => false,
        }
    }

    fn ask_to_trust_host_key(&mut self, source: Option<DataSourceId>, prompt: &HostKeyPrompt) {
        if !prompt.trust_allowed {
            self.notify(
                Level::Error,
                format!(
                    "{prompt}; '{}' only accepts known hosts — add the key to {} after verifying it",
                    prompt.server,
                    prompt.known_hosts.display()
                ),
            );
            return;
        }
        self.overlays.push(Overlay::Confirm(ConfirmState {
            title: format!("Trust SSH host {}:{}?", prompt.host, prompt.port),
            lines: vec![
                format!("Server:       {}", prompt.server),
                format!("Key type:     {}", prompt.algorithm),
                format!("Fingerprint:  {}", prompt.fingerprint),
                String::new(),
                "Only continue if this fingerprint matches the one published by the".into(),
                "server's administrator. It will be remembered in".into(),
                format!("{}.", prompt.known_hosts.display()),
            ],
            confirm_label: "Trust and connect".into(),
            danger: true,
            action: PendingAction::TrustHostKey {
                prompt: Box::new(prompt.clone()),
                retry: source,
            },
        }));
    }

    fn prompt_server_secret(
        &mut self,
        source: Option<DataSourceId>,
        server: ServerConfigId,
        name: &str,
        kind: CredentialKind,
    ) {
        self.overlays.push(Overlay::Prompt(PromptState {
            title: format!("SSH {} for {name}", kind.label()),
            hint: "kept for this session (saved only if the configuration saves secrets)".into(),
            input: LineInput::default(),
            masked: true,
            action: PromptAction::ServerSecret {
                server,
                kind: secret_kind(kind),
                retry: source,
            },
        }));
    }

    pub(crate) fn trust_host_key(&mut self, prompt: &HostKeyPrompt, retry: Option<DataSourceId>) {
        match self.context.network.trust_host_key(prompt) {
            Ok(()) => {
                self.notify(
                    Level::Success,
                    format!("trusted host key of {}", prompt.host),
                );
                self.retry_connection(retry);
            }
            Err(error) => self.notify(Level::Error, error.to_string()),
        }
    }

    pub(crate) fn approve_vpn(&mut self, profile: ServerConfigId, retry: Option<DataSourceId>) {
        self.vpn_approvals.insert(profile);
        self.retry_connection(retry);
    }

    pub(crate) fn submit_server_secret(
        &mut self,
        server: ServerConfigId,
        kind: ServerSecret,
        secret: String,
        retry: Option<DataSourceId>,
    ) {
        let secret = Secret::new(secret);
        let save = self
            .servers
            .ssh(server)
            .is_some_and(|config| config.save_secret);
        if save
            && let Some(key) = self.server_secret_key(server, kind)
            && let Err(error) = self.context.secrets.set(&key, &secret)
        {
            self.notify(
                Level::Warning,
                format!("the secret could not be stored: {error}"),
            );
        }
        self.server_secrets.insert((server, kind), secret);
        self.retry_connection(retry);
    }

    fn retry_connection(&mut self, retry: Option<DataSourceId>) {
        match retry {
            Some(source) => self.connect(source),
            None => self.notify(Level::Info, "press Ctrl+T to test again"),
        }
    }
}

fn secret_kind(kind: CredentialKind) -> ServerSecret {
    match kind {
        CredentialKind::Password => ServerSecret::Password,
        CredentialKind::Passphrase => ServerSecret::Passphrase,
    }
}
