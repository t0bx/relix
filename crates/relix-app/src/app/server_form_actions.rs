/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use crate::form::{FormCommand, FormStatus};
use crate::notifications::Level;
use crate::overlay::Overlay;
use crate::ssh_form::SshForm;
use crate::vpn_form::VpnForm;
use relix_config::KeyChord;
use relix_core::{ConfigScope, Secret, ServerConfigId, SshServerConfig, VpnProfile};
use relix_project::{ServerConfig, ServerSecret};

impl App {
    pub(crate) fn open_ssh_form(&mut self, existing: Option<ServerConfigId>) {
        let choices = self.server_choices().ssh;
        let project_open = !self.servers.is_read_only(ConfigScope::Project);
        let entry =
            existing.and_then(|id| self.servers.scope_of(id).zip(self.servers.ssh(id).cloned()));
        let form = SshForm::new(
            entry.as_ref().map(|(scope, config)| (*scope, config)),
            &choices,
            project_open,
        );
        self.overlays.push(Overlay::SshForm(Box::new(form)));
    }

    pub(crate) fn open_vpn_form(&mut self, existing: Option<ServerConfigId>) {
        let project_open = !self.servers.is_read_only(ConfigScope::Project);
        let entry =
            existing.and_then(|id| self.servers.scope_of(id).zip(self.servers.vpn(id).cloned()));
        let status = entry.as_ref().map_or_else(
            || "not saved".to_owned(),
            |(_, profile)| self.context.network.vpn_status(profile).label(),
        );
        let form = VpnForm::new(
            entry.as_ref().map(|(scope, profile)| (*scope, profile)),
            project_open,
            status,
        );
        self.overlays.push(Overlay::VpnForm(Box::new(form)));
    }

    pub(crate) fn ssh_form_key(
        &mut self,
        mut form: Box<SshForm>,
        chord: KeyChord,
    ) -> Option<Overlay> {
        match form.handle_key(chord) {
            FormCommand::Cancel => return None,
            FormCommand::Submit => match form.build() {
                Ok((scope, config, secret)) => match self.save_ssh_config(scope, config, secret) {
                    Ok(()) => return None,
                    Err(error) => form.set_status(FormStatus::failure(error), false),
                },
                Err(error) => form.set_status(FormStatus::failure(error), false),
            },
            FormCommand::Test => match form.build() {
                Ok((_, config, secret)) => match self.start_ssh_test(config, secret) {
                    Ok(()) => form.set_status(FormStatus::success("connecting…"), true),
                    Err(error) => form.set_status(FormStatus::failure(error), false),
                },
                Err(error) => form.set_status(FormStatus::failure(error), false),
            },
            _ => {}
        }
        Some(Overlay::SshForm(form))
    }

    pub(crate) fn vpn_form_key(
        &mut self,
        mut form: Box<VpnForm>,
        chord: KeyChord,
    ) -> Option<Overlay> {
        match form.handle_key(chord) {
            FormCommand::Cancel => return None,
            FormCommand::Submit => match form.build() {
                Ok((scope, profile)) => match self.save_vpn_profile(scope, profile) {
                    Ok(_) => return None,
                    Err(error) => form.set_status(FormStatus::failure(error), false),
                },
                Err(error) => form.set_status(FormStatus::failure(error), false),
            },
            FormCommand::Test => match form.build() {
                Ok((_, profile)) => {
                    form.set_status(FormStatus::success("checking…"), true);
                    self.diagnose_vpn(profile);
                }
                Err(error) => form.set_status(FormStatus::failure(error), false),
            },
            FormCommand::Shortcut(letter @ ('o' | 'x')) => match form.build() {
                Ok((scope, profile)) => match self.save_vpn_profile(scope, profile) {
                    Ok(id) => {
                        form.editing = Some(id);
                        form.set_status(FormStatus::success("saved"), true);
                        if letter == 'o' {
                            self.request_vpn_connect(id);
                        } else {
                            self.disconnect_vpn(id);
                        }
                    }
                    Err(error) => form.set_status(FormStatus::failure(error), false),
                },
                Err(error) => form.set_status(FormStatus::failure(error), false),
            },
            _ => {}
        }
        Some(Overlay::VpnForm(form))
    }

    /// Saves an SSH configuration after validating its jump host chain against the
    /// catalog. The secret goes to the credential store only when the configuration
    /// saves secrets; it is always remembered for this session.
    pub(crate) fn save_ssh_config(
        &mut self,
        scope: ConfigScope,
        config: SshServerConfig,
        secret: Option<Secret>,
    ) -> Result<(), String> {
        if self.servers.is_read_only(scope) {
            return Err(format!("{} configurations are read-only", scope.label()));
        }
        let id = config.id;
        let save_secret = config.save_secret;
        let mut candidate = self.servers.clone();
        candidate
            .registry_mut(scope)
            .upsert(ServerConfig::Ssh(config.clone()))
            .map_err(|error| error.to_string())?;
        candidate
            .jump_chain(id)
            .map_err(|error| error.to_string())?;
        self.servers = candidate;
        if !self.save_server_scope(scope) {
            return Err("the configuration could not be written".into());
        }
        let kind = match config.auth {
            relix_core::SshAuthMethod::Password => Some(ServerSecret::Password),
            relix_core::SshAuthMethod::PrivateKey => Some(ServerSecret::Passphrase),
            relix_core::SshAuthMethod::Agent => None,
        };
        for stale in [ServerSecret::Password, ServerSecret::Passphrase] {
            if (Some(stale) != kind || !save_secret)
                && let Some(key) = self.server_secret_key(id, stale)
            {
                let _ = self.context.secrets.delete(&key);
            }
            if Some(stale) != kind {
                self.server_secrets.remove(&(id, stale));
            }
        }
        if let (Some(kind), Some(secret)) = (kind, secret) {
            if save_secret && let Some(key) = self.server_secret_key(id, kind) {
                if let Err(error) = self.context.secrets.set(&key, &secret) {
                    self.notify(Level::Warning, format!("secret not stored: {error}"));
                } else if !self.context.secrets.is_persistent() {
                    self.notify(
                        Level::Warning,
                        "no OS keyring available: the secret is kept for this session only",
                    );
                }
            }
            self.server_secrets.insert((id, kind), secret);
        }
        self.notify(
            Level::Success,
            format!(
                "saved SSH configuration '{}'; connections pick it up on their next connect",
                config.name
            ),
        );
        Ok(())
    }

    pub(crate) fn save_vpn_profile(
        &mut self,
        scope: ConfigScope,
        profile: VpnProfile,
    ) -> Result<ServerConfigId, String> {
        if self.servers.is_read_only(scope) {
            return Err(format!("{} configurations are read-only", scope.label()));
        }
        let id = profile.id;
        let name = profile.name.clone();
        self.servers
            .registry_mut(scope)
            .upsert(ServerConfig::Vpn(profile))
            .map_err(|error| error.to_string())?;
        if !self.save_server_scope(scope) {
            return Err("the configuration could not be written".into());
        }
        self.notify(Level::Success, format!("saved VPN profile '{name}'"));
        Ok(id)
    }

    pub(crate) fn update_ssh_form_status(&mut self, status: FormStatus) {
        let form = self
            .overlays
            .iter_mut()
            .rev()
            .find_map(|overlay| match overlay {
                Overlay::SshForm(form) => Some(form),
                _ => None,
            });
        match form {
            Some(form) => form.set_status(status, false),
            None => self.notify(
                if status.success {
                    Level::Success
                } else {
                    Level::Error
                },
                status.message,
            ),
        }
    }

    pub(crate) fn update_vpn_form_status(&mut self, profile: ServerConfigId, status: FormStatus) {
        let connection = self
            .servers
            .vpn(profile)
            .map(|profile| self.context.network.vpn_status(profile).label());
        let form = self
            .overlays
            .iter_mut()
            .rev()
            .find_map(|overlay| match overlay {
                Overlay::VpnForm(form)
                    if form.editing.is_none() || form.editing == Some(profile) =>
                {
                    Some(form)
                }
                _ => None,
            });
        match form {
            Some(form) => {
                if let Some(connection) = connection {
                    form.connection_status = connection;
                }
                form.set_status(status, false);
            }
            None => self.notify(
                if status.success {
                    Level::Success
                } else {
                    Level::Error
                },
                status.message,
            ),
        }
    }
}
