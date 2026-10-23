/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use crate::datasource_form::DataSourceForm;
use crate::form::FormStatus;
use crate::notifications::Level;
use crate::overlay::{ConfirmState, Overlay, PendingAction};
use crate::tab::Tab;
use crate::workspace::TabId;
use relix_connections::{ConnectionError, NetworkPlan};
use relix_core::{DataSourceDefinition, DataSourceId, Secret};
use relix_driver_api::ServerInfo;

impl App {
    pub(crate) fn open_data_source_form(&mut self, existing: Option<DataSourceId>) {
        if self.project.is_none() {
            self.notify(Level::Info, "open a project first");
            return;
        }
        let drivers = self.context.drivers.descriptors();
        if drivers.is_empty() {
            self.notify(Level::Error, "no database drivers are available");
            return;
        }
        let definition = existing.and_then(|id| self.data_source(id).cloned());
        self.overlays
            .push(Overlay::DataSourceForm(Box::new(DataSourceForm::new(
                drivers,
                definition.as_ref(),
                self.server_choices(),
            ))));
    }

    /// Persists a data source. The password goes to the credential store only; project
    /// files never contain secrets.
    pub(crate) fn save_data_source(
        &mut self,
        definition: DataSourceDefinition,
        password: Option<Secret>,
    ) {
        let Some(project) = self.project.as_mut() else {
            return;
        };
        let id = definition.id;
        let name = definition.name.clone();
        let previous = project.data_source(id).cloned();
        if let Err(error) = project.upsert_data_source(definition.clone()) {
            self.notify(Level::Error, error.to_string());
            self.overlays
                .push(Overlay::DataSourceForm(Box::new(DataSourceForm::new(
                    self.context.drivers.descriptors(),
                    Some(&definition),
                    self.server_choices(),
                ))));
            return;
        }
        if project.settings.default_data_source.is_none() {
            project.settings.default_data_source = Some(id);
        }
        self.save_project();
        if let Some(key) = self.secret_key(id) {
            let result = match (&password, definition.settings.save_password) {
                (Some(password), true) => self.context.secrets.set(&key, password),
                (_, false) => self.context.secrets.delete(&key),
                (None, true) => Ok(()),
            };
            if let Err(error) = result {
                self.notify(
                    Level::Warning,
                    format!("password could not be stored: {error}"),
                );
            } else if password.is_some()
                && definition.settings.save_password
                && !self.context.secrets.is_persistent()
            {
                self.notify(
                    Level::Warning,
                    "no OS keyring available: the password is kept for this session only",
                );
            }
        }
        if let Some(password) = password.filter(|_| !definition.settings.save_password) {
            self.session_passwords.insert(id, password);
        }
        let reconnect = previous.is_some_and(|previous| previous != definition)
            && self.connection(id).is_some();
        if reconnect {
            self.disconnect(id);
            self.connect(id);
        }
        self.notify(Level::Success, format!("saved data source '{name}'"));
        if let Some(root) = self.explorer.root_for(id) {
            self.explorer.select_node(root);
        }
    }

    pub(crate) fn on_connection_tested(
        &mut self,
        result: Result<(ServerInfo, String), ConnectionError>,
        plan: Option<NetworkPlan>,
    ) {
        let status = match &result {
            Ok((info, route)) => FormStatus::success(format!(
                "✓ connected {route}: {} {}",
                info.product, info.version
            )),
            Err(error) => FormStatus::failure(format!("✗ {} — {error}", error.stage())),
        };
        if let Err(error) = &result {
            self.handle_network_failure(None, plan.as_ref(), error);
        }
        let form = self
            .overlays
            .iter_mut()
            .rev()
            .find_map(|overlay| match overlay {
                Overlay::DataSourceForm(form) => Some(form),
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

    pub(crate) fn duplicate_selected_data_source(&mut self) {
        let Some(source) = self.selected_explorer_source() else {
            return;
        };
        let Some(project) = self.project.as_mut() else {
            return;
        };
        if let Some(copy) = project.duplicate_data_source(source) {
            if let Some(settings) = project
                .data_source(copy)
                .map(|definition| definition.settings.clone())
                && settings.save_password
                && let (Some(from), Some(to)) = (self.secret_key(source), self.secret_key(copy))
                && let Ok(Some(secret)) = self.context.secrets.get(&from)
            {
                let _ = self.context.secrets.set(&to, &secret);
            }
            self.save_project();
            self.notify(Level::Success, "data source duplicated");
        }
    }

    pub(crate) fn confirm_delete_selected_data_source(&mut self) {
        let Some(source) = self.selected_explorer_source() else {
            return;
        };
        let Some(name) = self
            .data_source(source)
            .map(|definition| definition.name.clone())
        else {
            return;
        };
        self.overlays.push(Overlay::Confirm(ConfirmState {
            title: format!("Remove data source '{name}'?"),
            lines: vec![
                "The connection definition and its stored password are removed from this project."
                    .into(),
                "The database itself is not modified.".into(),
            ],
            confirm_label: "Remove".into(),
            danger: true,
            action: PendingAction::DeleteDataSource(source),
        }));
    }

    pub(crate) fn delete_data_source(&mut self, source: DataSourceId) {
        self.disconnect(source);
        if let Some(key) = self.secret_key(source) {
            let _ = self.context.secrets.delete(&key);
        }
        self.session_passwords.remove(&source);
        let orphaned: Vec<TabId> = self
            .workspace
            .tabs()
            .filter(|(_, tab)| {
                matches!(tab, Tab::Table(_) | Tab::KeyValue(_)) && tab.data_source() == Some(source)
            })
            .map(|(id, _)| id)
            .collect();
        for tab in orphaned {
            self.workspace.remove_tab(tab);
        }
        for (_, tab) in self.workspace.tabs_mut() {
            if let Tab::Editor(editor) = tab
                && editor.data_source == Some(source)
            {
                editor.data_source = None;
            }
        }
        if let Some(project) = self.project.as_mut() {
            project.remove_data_source(source);
        }
        self.save_project();
        self.ensure_focus_valid();
        self.notify(Level::Success, "data source removed");
    }
}
