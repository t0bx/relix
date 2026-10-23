/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use relix_connections::{Approval, HopSecrets, NetworkCredentials, NetworkPlan};
use relix_core::{Secret, ServerConfigId, SshAuthMethod};
use relix_project::{SecretKey, SecretStore, ServerSecret};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct HopSecretSource {
    cached: HopSecrets,
    password_key: Option<SecretKey>,
    passphrase_key: Option<SecretKey>,
}

#[derive(Debug, Clone)]
pub(crate) struct CredentialSources {
    hops: Vec<HopSecretSource>,
    vpn_approval: Option<Approval>,
}

impl App {
    /// Collects, per SSH hop, the secrets already entered in this session plus the
    /// keyring keys of configurations that save their secret. The keyring itself is read
    /// later on the blocking pool, never on the UI thread.
    pub(crate) fn credential_sources(&self, plan: &NetworkPlan) -> CredentialSources {
        let hops = plan
            .ssh_chain
            .iter()
            .map(|hop| {
                let key = |kind| {
                    hop.save_secret
                        .then(|| self.server_secret_key(hop.id, kind))
                        .flatten()
                };
                let (password_kind, passphrase_kind) = match hop.auth {
                    SshAuthMethod::Password => (Some(ServerSecret::Password), None),
                    SshAuthMethod::PrivateKey => (None, Some(ServerSecret::Passphrase)),
                    SshAuthMethod::Agent => (None, None),
                };
                HopSecretSource {
                    cached: HopSecrets {
                        password: password_kind
                            .and_then(|kind| self.cached_server_secret(hop.id, kind)),
                        passphrase: passphrase_kind
                            .and_then(|kind| self.cached_server_secret(hop.id, kind)),
                    },
                    password_key: password_kind.and_then(key),
                    passphrase_key: passphrase_kind.and_then(key),
                }
            })
            .collect();
        let vpn_approval = plan.vpn.as_ref().map(|profile| {
            if self.vpn_approvals.contains(&profile.id) {
                Approval::Granted
            } else {
                Approval::NotGranted
            }
        });
        CredentialSources { hops, vpn_approval }
    }

    pub(crate) fn server_secret_key(
        &self,
        server: ServerConfigId,
        kind: ServerSecret,
    ) -> Option<SecretKey> {
        let scope = self.servers.scope_of(server)?;
        SecretKey::for_server(scope, self.servers.project_id(), server, kind)
    }

    fn cached_server_secret(&self, server: ServerConfigId, kind: ServerSecret) -> Option<Secret> {
        self.server_secrets.get(&(server, kind)).cloned()
    }
}

pub(crate) async fn resolve_credentials(
    secrets: Arc<dyn SecretStore>,
    sources: CredentialSources,
) -> NetworkCredentials {
    let mut ssh = Vec::with_capacity(sources.hops.len());
    for hop in sources.hops {
        let password = match (hop.cached.password, hop.password_key) {
            (Some(secret), _) => Some(secret),
            (None, Some(key)) => load_secret(secrets.clone(), key).await,
            (None, None) => None,
        };
        let passphrase = match (hop.cached.passphrase, hop.passphrase_key) {
            (Some(secret), _) => Some(secret),
            (None, Some(key)) => load_secret(secrets.clone(), key).await,
            (None, None) => None,
        };
        ssh.push(HopSecrets {
            password,
            passphrase,
        });
    }
    NetworkCredentials {
        ssh,
        vpn_approval: sources.vpn_approval,
    }
}

pub(crate) async fn load_secret(secrets: Arc<dyn SecretStore>, key: SecretKey) -> Option<Secret> {
    tokio::task::spawn_blocking(move || secrets.get(&key))
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
}
