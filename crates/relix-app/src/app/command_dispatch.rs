/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::{App, Focus};
use crate::command::Command;
use crate::editor_tab::EditorFocus;
use crate::overlay::{Overlay, PromptAction};
use crate::tab::Tab;
use crate::workspace::Direction;
use relix_project::SplitDirection;

const PANE_RESIZE_STEP: i16 = 5;

impl App {
    pub fn execute_command(&mut self, command: Command) {
        match command {
            Command::Quit => self.request_quit(),
            Command::OpenPalette => self.open_palette(),
            Command::OpenProjects => self.open_project_picker(),
            Command::NewEditor => self.new_editor_for_context(),
            Command::Save => self.save_current(),
            Command::SaveAs => self.prompt_save_as(),
            Command::OpenFile => self.open_prompt(
                "Open SQL file",
                "path to a .sql file",
                PromptAction::OpenFile,
                "",
            ),
            Command::CloseTab => self.close_active_tab(false),
            Command::FocusNext => self.cycle_focus(true),
            Command::FocusPrevious => self.cycle_focus(false),
            Command::FocusExplorer => self.focus_explorer(),
            Command::ToggleSidebar => self.toggle_sidebar(),
            Command::ShowHelp => self.overlays.push(Overlay::Help { scroll: 0 }),
            Command::JumpMode => self.start_region_jump(),
            Command::SplitVertical => self.split_pane(SplitDirection::Vertical),
            Command::SplitHorizontal => self.split_pane(SplitDirection::Horizontal),
            Command::ClosePane => self.close_focused_pane(false),
            Command::FocusLeft => self.focus_pane_direction(Direction::Left),
            Command::FocusRight => self.focus_pane_direction(Direction::Right),
            Command::FocusUp => self.focus_pane_direction(Direction::Up),
            Command::FocusDown => self.focus_pane_direction(Direction::Down),
            Command::GrowPane => {
                self.workspace.resize_focused(PANE_RESIZE_STEP);
            }
            Command::ShrinkPane => {
                self.workspace.resize_focused(-PANE_RESIZE_STEP);
            }
            Command::NextTab => self.cycle_tab(true),
            Command::PreviousTab => self.cycle_tab(false),
            Command::SelectTab(number) => {
                self.workspace
                    .select_tab_index(usize::from(number.saturating_sub(1)));
                self.focus = Focus::Workspace;
            }
            Command::AddDataSource => self.open_data_source_form(None),
            Command::ServerConfigurations => self.open_server_manager(),
            Command::ShowDrivers => self.open_driver_manager(),
            Command::ShowHistory => self.open_history_picker(),
            Command::CycleTheme => self.cycle_theme(),
            Command::ExportProject => self.open_prompt(
                "Export project",
                "file path for the exported project (credentials are never included)",
                PromptAction::ExportProject,
                "relix-project.toml",
            ),
            Command::ImportProject => self.open_prompt(
                "Import project",
                "path to an exported project file",
                PromptAction::ImportProject,
                "",
            ),
            Command::ToggleConnection => self.toggle_connection_for_context(),
            Command::CancelQuery => self.cancel_running_query(),
            Command::RunStatement | Command::RunScript => self.run_command_in_context(command),
            _ => self.execute_focused_command(command),
        }
    }

    fn execute_focused_command(&mut self, command: Command) {
        if self.focus == Focus::Sidebar {
            self.explorer_command(command);
            return;
        }
        match self.active_tab() {
            Some(Tab::Editor(editor)) if editor.focus == EditorFocus::Text => {
                self.editor_command(command)
            }
            Some(Tab::Editor(_)) => self.results_command(command),
            Some(Tab::Table(_)) => self.table_command(command),
            Some(Tab::KeyValue(_)) => self.key_value_command(command),
            Some(Tab::Document(_)) => self.document_command(command),
            None => {
                if command == Command::Filter {
                    self.focus_explorer();
                    self.explorer.filtering = true;
                }
            }
        }
    }

    fn run_command_in_context(&mut self, command: Command) {
        let script = command == Command::RunScript;
        match (self.focus, self.active_tab()) {
            (Focus::Sidebar, _) => self.explorer_command(Command::Activate),
            (Focus::Workspace, Some(Tab::Editor(_))) => self.run_editor(script, false),
            (Focus::Workspace, Some(Tab::Table(_))) => self.reload_table_page(),
            (Focus::Workspace, Some(Tab::KeyValue(_))) => self.submit_console_command(),
            (Focus::Workspace, Some(Tab::Document(_))) => self.document_command(Command::Refresh),
            (Focus::Workspace, None) => {}
        }
    }

    fn new_editor_for_context(&mut self) {
        let source = match self.focus {
            Focus::Sidebar => self.selected_explorer_source(),
            Focus::Workspace => self.active_tab().and_then(Tab::data_source),
        }
        .or_else(|| {
            self.project
                .as_ref()
                .and_then(|project| project.settings.default_data_source)
        });
        self.new_editor_tab(source, "");
    }

    fn cycle_tab(&mut self, forward: bool) {
        self.workspace.cycle_tab(forward);
        self.focus = Focus::Workspace;
    }

    fn cycle_theme(&mut self) {
        let names: Vec<String> = self.themes.names().into_iter().map(str::to_owned).collect();
        let index = names
            .iter()
            .position(|name| *name == self.theme.name)
            .map_or(0, |index| (index + 1) % names.len().max(1));
        if let Some(name) = names.get(index).cloned() {
            self.apply_theme(&name);
        }
    }

    pub(crate) fn apply_theme(&mut self, name: &str) {
        let mut theme = self.themes.resolve(name);
        theme.apply_overrides(&self.settings.theme_overrides);
        self.theme = theme;
        self.settings.theme = self.theme.name.clone();
        if let Err(error) = self.settings.save(&self.context.paths.settings_file()) {
            self.notify(
                crate::notifications::Level::Error,
                format!("could not save settings: {error}"),
            );
        }
        self.status = format!("theme: {}", self.theme.name);
    }

    fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
        if !self.sidebar_visible && self.focus == Focus::Sidebar {
            self.focus = Focus::Workspace;
        }
    }
}
