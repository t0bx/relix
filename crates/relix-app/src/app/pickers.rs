/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use crate::command::COMMANDS;
use crate::input::LineInput;
use crate::notifications::Level;
use crate::overlay::{
    Overlay, PickerItem, PickerPurpose, PickerState, PickerValue, PromptAction, PromptState,
};
use crate::tab::Tab;
use chrono::Local;

impl App {
    pub(crate) fn open_prompt(
        &mut self,
        title: &str,
        hint: &str,
        action: PromptAction,
        initial: &str,
    ) {
        self.overlays.push(Overlay::Prompt(PromptState {
            title: title.to_owned(),
            hint: hint.to_owned(),
            input: LineInput::with_text(initial),
            masked: false,
            action,
        }));
    }

    pub(crate) fn open_palette(&mut self) {
        let mut items: Vec<PickerItem> = COMMANDS
            .iter()
            .filter(|spec| spec.in_palette)
            .map(|spec| PickerItem {
                label: spec.title.to_owned(),
                detail: spec.id.to_owned(),
                hint: self.keymap.primary_key(spec.command).unwrap_or_default(),
                value: PickerValue::Command(spec.command),
            })
            .collect();
        items.dedup_by(|a, b| a.value == b.value);
        items.extend(self.themes.names().into_iter().map(|name| PickerItem {
            label: format!("Theme: {name}"),
            detail: "theme".into(),
            hint: if name == self.theme.name {
                "active".into()
            } else {
                String::new()
            },
            value: PickerValue::Theme(name.to_owned()),
        }));
        self.overlays.push(Overlay::Picker(PickerState::new(
            "Command palette",
            PickerPurpose::Palette,
            items,
            "type to search · Enter run · Esc close",
        )));
    }

    pub(crate) fn open_project_picker(&mut self) {
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
        let mut projects = listing.projects;
        projects.sort_by_key(|project| self.recent.position(project.id).unwrap_or(usize::MAX));
        let current = self.project.as_ref().map(|project| project.id);
        let items = projects
            .into_iter()
            .map(|project| PickerItem {
                detail: format!("{} data sources", project.data_sources.len()),
                hint: if Some(project.id) == current {
                    "current".into()
                } else {
                    String::new()
                },
                label: project.name,
                value: PickerValue::Project(project.id),
            })
            .collect();
        self.overlays.push(Overlay::Picker(PickerState::new(
            "Projects",
            PickerPurpose::Projects,
            items,
            "Enter open · Ctrl+N new · Ctrl+R rename · Ctrl+D delete · Esc close",
        )));
    }

    pub(crate) fn open_editor_data_source_picker(&mut self) {
        let Some(project) = &self.project else {
            return;
        };
        let items: Vec<PickerItem> = project
            .data_sources
            .iter()
            .filter(|source| {
                self.capabilities(source.id)
                    .is_none_or(|capabilities| capabilities.query_execution)
            })
            .map(|source| PickerItem {
                label: source.name.clone(),
                detail: self.driver_name(&source.driver),
                hint: source.environment.label().to_owned(),
                value: PickerValue::DataSource(source.id),
            })
            .collect();
        if items.is_empty() {
            self.notify(Level::Info, "add a SQL data source first (Ctrl+K a)");
            return;
        }
        self.overlays.push(Overlay::Picker(PickerState::new(
            "Data source for this query tab",
            PickerPurpose::EditorDataSource,
            items,
            "Enter choose · Esc cancel",
        )));
    }

    pub(crate) fn open_history_picker(&mut self) {
        let names: std::collections::HashMap<_, _> = self
            .project
            .as_ref()
            .map(|project| {
                project
                    .data_sources
                    .iter()
                    .map(|source| (source.id, source.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let items: Vec<PickerItem> = self
            .history
            .newest_first()
            .enumerate()
            .map(|(index, entry)| PickerItem {
                label: entry.sql.split_whitespace().collect::<Vec<_>>().join(" "),
                detail: format!(
                    "{} · {} · {}",
                    entry
                        .data_source
                        .and_then(|id| names.get(&id).cloned())
                        .unwrap_or_else(|| "—".into()),
                    entry
                        .executed_at
                        .with_timezone(&Local)
                        .format("%Y-%m-%d %H:%M"),
                    entry.summary
                ),
                hint: if entry.success {
                    format!("{} ms", entry.duration_ms)
                } else {
                    "failed".into()
                },
                value: PickerValue::History(index),
            })
            .collect();
        if items.is_empty() {
            self.status = "query history is empty".into();
            return;
        }
        self.overlays.push(Overlay::Picker(PickerState::new(
            "Query history",
            PickerPurpose::History,
            items,
            "Enter insert into editor · Ctrl+R run again · Esc close",
        )));
    }

    pub(crate) fn accept_picker(&mut self, picker: PickerState, alternate: bool) {
        let Some(item) = picker.selected_item().cloned() else {
            return;
        };
        match item.value {
            PickerValue::Command(command) => self.execute_command(command),
            PickerValue::Theme(name) => self.apply_theme(&name),
            PickerValue::Project(id) => self.request_switch_project(id),
            PickerValue::DataSource(source) => {
                if let Some(Tab::Editor(editor)) = self.active_tab_mut() {
                    editor.data_source = Some(source);
                    editor.in_transaction = false;
                }
                if self.connection(source).is_none() {
                    self.connect(source);
                }
            }
            PickerValue::History(index) => self.use_history_entry(index, alternate),
            PickerValue::Driver(id) => self.driver_selected(&id),
            PickerValue::Filter(filter) => self.apply_filter_from_history(&filter),
            PickerValue::DocumentIndex { tab, name } => self.confirm_drop_document_index(tab, name),
            PickerValue::Export(format) => {
                let Some(tab) = self.workspace.active_tab_id() else {
                    return;
                };
                let name = self
                    .active_tab()
                    .map(Tab::title)
                    .unwrap_or_else(|| "results".into());
                let name: String = name
                    .chars()
                    .filter(|ch| ch.is_alphanumeric() || *ch == '_')
                    .collect();
                self.open_prompt(
                    "Export to file",
                    "path of the file to write",
                    PromptAction::ExportResults { tab, format },
                    &format!("{name}.{}", format.extension()),
                );
            }
        }
    }

    fn use_history_entry(&mut self, index: usize, run: bool) {
        let Some(entry) = self.history.newest_first().nth(index).cloned() else {
            return;
        };
        if run {
            let tab = self.new_editor_tab(entry.data_source, &entry.sql);
            self.workspace.activate_tab(tab);
            self.run_editor(false, false);
        } else {
            let mut text = entry.sql;
            if !text.trim_end().ends_with(';') {
                text.push(';');
            }
            text.push('\n');
            self.insert_into_editor(&text);
            if let (Some(source), Some(Tab::Editor(editor))) =
                (entry.data_source, self.active_tab_mut())
                && editor.data_source.is_none()
            {
                editor.data_source = Some(source);
            }
        }
    }
}
