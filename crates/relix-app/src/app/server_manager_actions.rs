/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use crate::notifications::Level;
use crate::overlay::{ConfirmState, Overlay, PendingAction, PromptAction};
use crate::server_manager::{ServerKind, ServerManagerState, ServerRow};
use relix_config::{KeyChord, KeyCode};
use relix_core::{ConfigScope, ServerConfigId};
use relix_project::{SecretKey, ServerConfig, ServerSecret, TransferMode};

impl App {
    pub(crate) fn open_server_manager(&mut self) {
        self.overlays
            .push(Overlay::ServerManager(ServerManagerState::default()));
    }

    pub fn server_rows(&self) -> Vec<ServerRow> {
        let sources = self
            .project
            .as_ref()
            .map(|project| project.data_sources.as_slice())
            .unwrap_or_default();
        let sessions = self.context.network.ssh_sessions();
        self.servers
            .entries()
            .into_iter()
            .map(|entry| {
                let id = entry.config.id();
                let usages = self.servers.usages(id, sources).len();
                match &entry.config {
                    ServerConfig::Ssh(config) => {
                        let endpoint = format!("{}:{}", config.host, config.port);
                        let active = sessions
                            .iter()
                            .filter(|session| session.endpoint == endpoint)
                            .map(|session| session.status.label())
                            .next();
                        ServerRow {
                            id,
                            kind: ServerKind::Ssh,
                            scope: entry.scope,
                            name: config.name.clone(),
                            detail: format!(
                                "{} · {}",
                                config.endpoint_label(),
                                config.auth.label()
                            ),
                            status: active.unwrap_or_else(|| "idle".into()),
                            usages,
                        }
                    }
                    ServerConfig::Vpn(profile) => ServerRow {
                        id,
                        kind: ServerKind::Vpn,
                        scope: entry.scope,
                        name: profile.name.clone(),
                        detail: format!("{} · {}", profile.provider.label(), profile.mode.label()),
                        status: self.context.network.vpn_status(profile).label(),
                        usages,
                    },
                }
            })
            .collect()
    }

    pub(crate) fn server_manager_key(
        &mut self,
        mut state: ServerManagerState,
        chord: KeyChord,
    ) -> Option<Overlay> {
        let rows = self.server_rows();
        state.clamp(rows.len());
        let selected = rows.get(state.selected).cloned();
        let base = self.overlays.len();
        match (chord.code, selected) {
            (KeyCode::Esc | KeyCode::Char('q'), _) => return None,
            (KeyCode::Up | KeyCode::Char('k'), _) => state.move_selection(-1, rows.len()),
            (KeyCode::Down | KeyCode::Char('j'), _) => state.move_selection(1, rows.len()),
            (KeyCode::Char('n'), _) => self.open_ssh_form(None),
            (KeyCode::Char('v'), _) => self.open_vpn_form(None),
            (KeyCode::Enter | KeyCode::Char('e'), Some(row)) => match row.kind {
                ServerKind::Ssh => self.open_ssh_form(Some(row.id)),
                ServerKind::Vpn => self.open_vpn_form(Some(row.id)),
            },
            (KeyCode::Char('d') | KeyCode::Delete, Some(row)) => self.confirm_delete_server(&row),
            (KeyCode::Char('y'), Some(row)) => self.duplicate_server(&row),
            (KeyCode::Char('r'), Some(row)) => self.open_prompt(
                "Rename configuration",
                "new name (references stay valid)",
                PromptAction::RenameServerConfig(row.id),
                &row.name,
            ),
            (KeyCode::Char('t'), Some(row)) => self.test_server(&row),
            (KeyCode::Char('m'), Some(row)) => self.confirm_transfer(&row, TransferMode::Move),
            (KeyCode::Char('c'), Some(row)) => self.confirm_transfer(&row, TransferMode::Copy),
            (KeyCode::Char(' '), Some(row)) if row.kind == ServerKind::Vpn => {
                self.toggle_vpn(row.id);
            }
            _ => {}
        }
        self.overlays
            .insert(base.min(self.overlays.len()), Overlay::ServerManager(state));
        None
    }

    fn confirm_delete_server(&mut self, row: &ServerRow) {
        let dependents = self.servers.dependent_ssh_configs(row.id).len();
        let mut lines = vec![format!(
            "{} {} configuration ({}).",
            row.scope.label(),
            row.kind.label(),
            row.detail
        )];
        if row.usages > 0 {
            lines.push(format!(
                "{} data source(s) in this project use it and will need another configuration.",
                row.usages
            ));
        }
        if dependents > 0 {
            lines.push(format!(
                "{dependents} SSH configuration(s) use it as jump host."
            ));
        }
        lines.push("Stored secrets of this configuration are removed as well.".into());
        self.overlays.push(Overlay::Confirm(ConfirmState {
            title: format!("Delete '{}'?", row.name),
            lines,
            confirm_label: "Delete".into(),
            danger: true,
            action: PendingAction::DeleteServerConfig(row.id),
        }));
    }

    pub(crate) fn delete_server_config(&mut self, id: ServerConfigId) {
        let Some(scope) = self.servers.scope_of(id) else {
            return;
        };
        for kind in [ServerSecret::Password, ServerSecret::Passphrase] {
            if let Some(key) = self.server_secret_key(id, kind) {
                let _ = self.context.secrets.delete(&key);
            }
            self.server_secrets.remove(&(id, kind));
        }
        let removed = self.servers.registry_mut(scope).remove(id);
        if self.save_server_scope(scope)
            && let Some(config) = removed
        {
            self.notify(Level::Success, format!("deleted '{}'", config.name()));
        }
    }

    fn duplicate_server(&mut self, row: &ServerRow) {
        let copy = self.servers.registry_mut(row.scope).duplicate(row.id);
        if copy.is_some() && self.save_server_scope(row.scope) {
            self.notify(
                Level::Success,
                format!("duplicated '{}' (secrets are not copied)", row.name),
            );
        }
    }

    pub(crate) fn rename_server_config(&mut self, id: ServerConfigId, name: &str) {
        let Some(scope) = self.servers.scope_of(id) else {
            return;
        };
        match self.servers.registry_mut(scope).rename(id, name) {
            Ok(()) => {
                if self.save_server_scope(scope) {
                    self.notify(Level::Success, "configuration renamed");
                }
            }
            Err(error) => self.notify(Level::Error, error.to_string()),
        }
    }

    fn confirm_transfer(&mut self, row: &ServerRow, mode: TransferMode) {
        let target = row.scope.other();
        if self.servers.is_read_only(target) {
            self.notify(
                Level::Error,
                format!("{} configurations are not available here", target.label()),
            );
            return;
        }
        let verb = match mode {
            TransferMode::Move => "Move",
            TransferMode::Copy => "Copy",
        };
        let mut lines = vec![format!(
            "{verb} '{}' from {} to {} configurations.",
            row.name,
            row.scope.label(),
            target.label()
        )];
        lines.push(match (mode, target) {
            (TransferMode::Move, ConfigScope::Global) => {
                "It becomes available to every project, including its stored secret.".into()
            }
            (TransferMode::Move, ConfigScope::Project) => {
                "Other projects that use it will no longer find it.".into()
            }
            (TransferMode::Copy, _) => {
                "The copy gets its own identity; stored secrets are not copied.".into()
            }
        });
        self.overlays.push(Overlay::Confirm(ConfirmState {
            title: format!("{verb} configuration?"),
            lines,
            confirm_label: verb.into(),
            danger: false,
            action: PendingAction::TransferServerConfig { id: row.id, mode },
        }));
    }

    pub(crate) fn transfer_server_config(&mut self, id: ServerConfigId, mode: TransferMode) {
        let Some(from) = self.servers.scope_of(id) else {
            return;
        };
        let to = from.other();
        let secrets = (mode == TransferMode::Move).then(|| self.read_server_secrets(id));
        let Some(new_id) = self.servers.transfer(id, to, mode) else {
            return;
        };
        if !(self.save_server_scope(to) && self.save_server_scope(from)) {
            return;
        }
        if let Some(secrets) = secrets {
            for (kind, old_key, secret) in secrets {
                if let Some(new_key) = self.server_secret_key(new_id, kind) {
                    let _ = self.context.secrets.set(&new_key, &secret);
                    let _ = self.context.secrets.delete(&old_key);
                }
            }
        }
        self.notify(
            Level::Success,
            format!("configuration is now {}", to.label()),
        );
    }

    fn read_server_secrets(
        &self,
        id: ServerConfigId,
    ) -> Vec<(ServerSecret, SecretKey, relix_core::Secret)> {
        [ServerSecret::Password, ServerSecret::Passphrase]
            .into_iter()
            .filter_map(|kind| {
                let key = self.server_secret_key(id, kind)?;
                let secret = self.context.secrets.get(&key).ok().flatten()?;
                Some((kind, key, secret))
            })
            .collect()
    }
}
