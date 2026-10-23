/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::{App, Focus};
use crate::explorer::ExplorerState;
use crate::notifications::Level;
use crate::overlay::{ConfirmState, Overlay, PendingAction, PickerState, PromptAction};
use crate::workspace::Workspace;
use relix_config::{KeyChord, KeyCode};
use relix_core::ProjectId;
use relix_project::portable::{export_project, import_project};
use relix_project::{Project, SecretKey};
use relix_query::{HistoryEntry, QueryHistory};

const DEFAULT_PROJECT_NAME: &str = "Development";

impl App {
    /// Opens the requested project (by name), otherwise the most recently used one, and
    /// creates a first project when none exist yet.
    pub fn startup(&mut self, requested: Option<&str>) {
        let listing = match self.context.store.list() {
            Ok(listing) => listing,
            Err(error) => {
                self.notify(Level::Error, error.to_string());
                return;
            }
        };
        for warning in &listing.warnings {
            self.notify(Level::Warning, warning.clone());
        }
        let target = match requested {
            Some(name) => match listing
                .projects
                .iter()
                .find(|project| project.name.eq_ignore_ascii_case(name))
            {
                Some(project) => Some(project.id),
                None => match self.context.store.create(name) {
                    Ok(project) => {
                        self.notify(
                            Level::Success,
                            format!("created project '{}'", project.name),
                        );
                        Some(project.id)
                    }
                    Err(error) => {
                        self.notify(Level::Error, error.to_string());
                        None
                    }
                },
            },
            None => self
                .recent
                .most_recent()
                .filter(|id| listing.projects.iter().any(|project| project.id == *id))
                .or_else(|| listing.projects.first().map(|project| project.id)),
        };
        let target = target.or_else(|| {
            self.context
                .store
                .create(DEFAULT_PROJECT_NAME)
                .ok()
                .map(|project| project.id)
        });
        if let Some(id) = target {
            self.open_project(id);
        }
    }

    pub(crate) fn request_switch_project(&mut self, id: ProjectId) {
        if self
            .project
            .as_ref()
            .is_some_and(|project| project.id == id)
        {
            return;
        }
        let pending = self
            .workspace
            .tabs()
            .any(|(_, tab)| tab.has_pending_changes());
        if pending {
            self.overlays.push(Overlay::Confirm(ConfirmState {
                title: "Switch project?".into(),
                lines: vec![
                    "Uncommitted data changes in open tables will be lost.".into(),
                    "Unsaved scripts are kept as drafts with the current project.".into(),
                ],
                confirm_label: "Switch".into(),
                danger: true,
                action: PendingAction::SwitchProject(id),
            }));
            return;
        }
        self.switch_project(id);
    }

    pub(crate) fn switch_project(&mut self, id: ProjectId) {
        if let Err(error) = self.save_session() {
            self.notify(
                Level::Error,
                format!("could not save the current session: {error}"),
            );
        }
        self.disconnect_all();
        self.open_project(id);
    }

    fn open_project(&mut self, id: ProjectId) {
        let project = match self.context.store.load(id) {
            Ok(project) => project,
            Err(error) => {
                self.notify(Level::Error, error.to_string());
                return;
            }
        };
        self.recent.touch(id);
        if let Err(error) = self.recent.save(&self.context.paths.recent_projects_file()) {
            tracing::warn!(%error, "failed to save recent projects");
        }
        let history: Vec<HistoryEntry> = self
            .context
            .store
            .load_history(id, self.settings.query.history_limit)
            .unwrap_or_default();
        self.history = QueryHistory::from_entries(history, self.settings.query.history_limit);
        self.filter_history = self.context.store.load_filters(id).unwrap_or_default();
        self.workspace = Workspace::new();
        self.explorer = ExplorerState::default();
        self.connections.clear();
        self.catalogs.clear();
        self.session_passwords.clear();
        self.project = Some(project);
        self.restore_session();
        self.refresh_explorer_roots();
        self.status = format!(
            "project {}",
            self.project
                .as_ref()
                .map(|project| project.name.as_str())
                .unwrap_or_default()
        );
    }

    pub(crate) fn project_picker_shortcut(&mut self, picker: &PickerState, chord: KeyChord) {
        let selected = picker.selected_item().and_then(|item| match &item.value {
            crate::overlay::PickerValue::Project(id) => Some((*id, item.label.clone())),
            _ => None,
        });
        match (chord.code, selected) {
            (KeyCode::Char('n'), _) => {
                self.open_prompt("New project", "project name", PromptAction::NewProject, "")
            }
            (KeyCode::Char('r'), Some((id, name))) => {
                self.open_prompt(
                    "Rename project",
                    "new name",
                    PromptAction::RenameProject(id),
                    &name,
                );
            }
            (KeyCode::Char('d'), Some((id, name))) => {
                self.overlays.push(Overlay::Confirm(ConfirmState {
                    title: format!("Delete project '{name}'?"),
                    lines: vec![
                        "This removes the project, its data source definitions, saved scripts,"
                            .into(),
                        "drafts and query history from this machine. Databases are not touched."
                            .into(),
                    ],
                    confirm_label: "Delete project".into(),
                    danger: true,
                    action: PendingAction::DeleteProject(id),
                }));
            }
            _ => {}
        }
    }

    pub(crate) fn create_project(&mut self, name: &str) {
        match self.context.store.create(name) {
            Ok(project) => {
                self.notify(
                    Level::Success,
                    format!("created project '{}'", project.name),
                );
                self.request_switch_project(project.id);
            }
            Err(error) => self.notify(Level::Error, error.to_string()),
        }
    }

    pub(crate) fn rename_project(&mut self, id: ProjectId, name: &str) {
        match self.context.store.rename(id, name) {
            Ok(renamed) => {
                if let Some(project) = self.project.as_mut().filter(|project| project.id == id) {
                    project.name = renamed.name.clone();
                }
                self.notify(Level::Success, format!("renamed to '{}'", renamed.name));
            }
            Err(error) => self.notify(Level::Error, error.to_string()),
        }
    }

    pub(crate) fn delete_project(&mut self, id: ProjectId) {
        if self
            .project
            .as_ref()
            .is_some_and(|project| project.id == id)
        {
            self.notify(
                Level::Warning,
                "switch to another project before deleting this one",
            );
            return;
        }
        let sources = self
            .context
            .store
            .load(id)
            .map(|project| project.data_sources)
            .unwrap_or_default();
        match self.context.store.delete(id) {
            Ok(()) => {
                for source in sources {
                    let _ = self
                        .context
                        .secrets
                        .delete(&SecretKey::for_data_source(id, source.id));
                }
                self.recent.remove(id);
                let _ = self.recent.save(&self.context.paths.recent_projects_file());
                self.notify(Level::Success, "project deleted");
            }
            Err(error) => self.notify(Level::Error, error.to_string()),
        }
    }

    pub(crate) fn save_project(&mut self) {
        let Some(project) = &self.project else {
            return;
        };
        if let Err(error) = self.context.store.save(project) {
            self.notify(Level::Error, format!("could not save project: {error}"));
        }
        self.refresh_explorer_roots();
    }

    pub(crate) fn export_project(&mut self, path: &str) {
        let Some(project) = &self.project else {
            return;
        };
        let result = export_project(project)
            .map_err(|error| error.to_string())
            .and_then(|text| std::fs::write(path, text).map_err(|error| error.to_string()));
        match result {
            Ok(()) => self.notify(
                Level::Success,
                format!("exported project to {path} (no credentials)"),
            ),
            Err(error) => self.notify(Level::Error, format!("export failed: {error}")),
        }
    }

    pub(crate) fn import_project(&mut self, path: &str) {
        let result = std::fs::read_to_string(path)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                import_project(&self.context.store, &text, None).map_err(|error| error.to_string())
            });
        match result {
            Ok(project) => {
                self.notify(
                    Level::Success,
                    format!("imported project '{}'", project.name),
                );
                self.request_switch_project(project.id);
            }
            Err(error) => self.notify(Level::Error, format!("import failed: {error}")),
        }
    }

    pub fn project_name(&self) -> Option<&str> {
        self.project.as_ref().map(|project| project.name.as_str())
    }

    pub fn current_project(&self) -> Option<&Project> {
        self.project.as_ref()
    }

    pub(crate) fn ensure_focus_valid(&mut self) {
        if self.focus == Focus::Workspace
            && self.workspace.active_tab().is_none()
            && self.sidebar_visible
        {
            self.focus = Focus::Sidebar;
        }
    }
}
