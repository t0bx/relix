/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::{App, Focus, HitTarget};
use crate::editor_tab::{EditorFocus, EditorTab};
use crate::events::{MouseInput, MouseKind};
use crate::keyvalue_tab::KeyValueTab;
use crate::notifications::Level;
use crate::overlay::{ConfirmState, JumpState, JumpTarget, Overlay, PendingAction, jump_labels};
use crate::tab::Tab;
use crate::workspace::{Direction, TabId};
use relix_config::{KeyChord, KeyCode};
use relix_project::SplitDirection;

const MOUSE_SCROLL_LINES: isize = 3;

impl App {
    pub(crate) fn focus_explorer(&mut self) {
        self.sidebar_visible = true;
        self.focus = Focus::Sidebar;
    }

    pub(crate) fn focus_workspace(&mut self) {
        self.focus = Focus::Workspace;
    }

    /// Cycles focus through the sidebar, every pane and, inside editor tabs, between the
    /// text and its results.
    pub(crate) fn cycle_focus(&mut self, forward: bool) {
        let panes = self.workspace.pane_order();
        let in_editor_results = matches!(
            self.active_tab(),
            Some(Tab::Editor(editor)) if editor.focus == EditorFocus::Results
        );
        let editor_has_results = matches!(
            self.active_tab(),
            Some(Tab::Editor(editor)) if !editor.results.is_empty()
        );
        if self.focus == Focus::Workspace && forward && editor_has_results && !in_editor_results {
            self.set_editor_focus(EditorFocus::Results);
            return;
        }
        if self.focus == Focus::Workspace && !forward && in_editor_results {
            self.set_editor_focus(EditorFocus::Text);
            return;
        }
        let mut stops: Vec<Option<u32>> = Vec::new();
        if self.sidebar_visible {
            stops.push(None);
        }
        stops.extend(panes.iter().map(|pane| Some(*pane)));
        let current = match self.focus {
            Focus::Sidebar => None,
            Focus::Workspace => Some(self.workspace.focused_pane()),
        };
        let position = stops.iter().position(|stop| *stop == current).unwrap_or(0);
        let count = stops.len().max(1);
        let next = if forward {
            (position + 1) % count
        } else {
            (position + count - 1) % count
        };
        match stops.get(next).copied().flatten() {
            Some(pane) => {
                self.workspace.focus_pane(pane);
                self.focus = Focus::Workspace;
                let entering_at_results = !forward && editor_has_results;
                self.set_editor_focus(if entering_at_results {
                    EditorFocus::Results
                } else {
                    EditorFocus::Text
                });
            }
            None => self.focus = Focus::Sidebar,
        }
    }

    pub(crate) fn set_editor_focus(&mut self, focus: EditorFocus) {
        if let Some(Tab::Editor(editor)) = self.active_tab_mut() {
            editor.focus = if focus == EditorFocus::Results && editor.results.is_empty() {
                EditorFocus::Text
            } else {
                focus
            };
            editor.completion = None;
        }
    }

    pub(crate) fn focus_pane_direction(&mut self, direction: Direction) {
        if self.focus == Focus::Sidebar {
            if direction == Direction::Right {
                self.focus = Focus::Workspace;
            }
            return;
        }
        let moved = self.workspace.focus_direction(direction);
        if !moved && direction == Direction::Left && self.sidebar_visible {
            self.focus = Focus::Sidebar;
        }
    }

    pub(crate) fn split_pane(&mut self, direction: SplitDirection) {
        let duplicate = match self.active_tab() {
            Some(Tab::Table(table)) => Some(Tab::Table(Box::new(table.duplicate()))),
            Some(Tab::KeyValue(tab)) => {
                Some(Tab::KeyValue(Box::new(KeyValueTab::new(tab.data_source))))
            }
            Some(Tab::Editor(editor)) => Some(Tab::Editor(Box::new(EditorTab::new(
                self.next_console_title(),
                editor.data_source,
                "",
            )))),
            None => None,
        };
        let pane = self.workspace.split(direction);
        self.focus = Focus::Workspace;
        if let Some(tab) = duplicate {
            let id = self.workspace.add_tab(pane, tab);
            self.load_tab_content(id);
        }
    }

    pub(crate) fn load_tab_content(&mut self, tab: TabId) {
        match self.workspace.tab(tab) {
            Some(Tab::Table(_)) => self.load_table(tab),
            Some(Tab::KeyValue(_)) => self.start_key_scan(tab, true),
            _ => {}
        }
    }

    pub(crate) fn close_active_tab(&mut self, force: bool) {
        let Some(tab) = self.workspace.active_tab_id() else {
            return;
        };
        self.close_tab(tab, force);
    }

    pub(crate) fn close_tab(&mut self, tab: TabId, force: bool) {
        let Some(existing) = self.workspace.tab(tab) else {
            return;
        };
        let unsaved = matches!(existing, Tab::Editor(editor) if editor.is_dirty());
        if !force && (unsaved || existing.has_pending_changes()) {
            let what = if unsaved {
                "unsaved script changes"
            } else {
                "uncommitted data changes"
            };
            self.overlays.push(Overlay::Confirm(ConfirmState {
                title: format!("Close '{}'?", existing.title()),
                lines: vec![format!("This tab has {what} that will be lost.")],
                confirm_label: "Discard & close".into(),
                danger: true,
                action: PendingAction::CloseTab(tab),
            }));
            return;
        }
        if let Some(Tab::Editor(editor)) = self.workspace.tab(tab)
            && let Some(running) = &editor.running
        {
            running.cancel.cancel();
        }
        self.workspace.remove_tab(tab);
    }

    pub(crate) fn close_focused_pane(&mut self, force: bool) {
        let pane_id = self.workspace.focused_pane();
        let blocking = self
            .workspace
            .pane(pane_id)
            .map(|pane| {
                pane.tabs.iter().any(|tab| {
                    self.workspace.tab(*tab).is_some_and(|tab| {
                        tab.has_pending_changes()
                            || matches!(tab, Tab::Editor(editor) if editor.is_dirty())
                    })
                })
            })
            .unwrap_or(false);
        if blocking && !force {
            self.overlays.push(Overlay::Confirm(ConfirmState {
                title: "Close pane?".into(),
                lines: vec!["Some tabs in this pane have unsaved or uncommitted changes.".into()],
                confirm_label: "Discard & close".into(),
                danger: true,
                action: PendingAction::ClosePane(pane_id),
            }));
            return;
        }
        for tab in self.workspace.close_pane(pane_id) {
            if let Tab::Editor(editor) = tab
                && let Some(running) = editor.running
            {
                running.cancel.cancel();
            }
        }
    }

    /// Labels the sidebar, every pane and the tabs of the focused pane for quick jumping.
    pub(crate) fn start_region_jump(&mut self) {
        let mut targets = Vec::new();
        if self.sidebar_visible {
            targets.push(JumpTarget::Sidebar);
        }
        for pane in self.workspace.pane_order() {
            targets.push(JumpTarget::Pane(pane));
            if let Some(pane) = self.workspace.pane(pane) {
                targets.extend(pane.tabs.iter().map(|tab| JumpTarget::Tab(*tab)));
            }
        }
        self.start_jump(targets);
    }

    pub(crate) fn start_jump(&mut self, targets: Vec<JumpTarget>) {
        if targets.is_empty() {
            return;
        }
        let labels = jump_labels(targets.len());
        self.overlays.push(Overlay::Jump(JumpState {
            targets: labels.into_iter().zip(targets).collect(),
            typed: String::new(),
        }));
    }

    pub(crate) fn perform_jump(&mut self, target: JumpTarget) {
        match target {
            JumpTarget::Sidebar => self.focus_explorer(),
            JumpTarget::Pane(pane) => {
                self.workspace.focus_pane(pane);
                self.focus = Focus::Workspace;
            }
            JumpTarget::Tab(tab) => {
                self.workspace.activate_tab(tab);
                self.focus = Focus::Workspace;
            }
            JumpTarget::Row(row) => {
                if let Some(grid) = self.active_tab_mut().and_then(Tab::focused_grid_mut) {
                    grid.move_to_row(row);
                }
            }
            JumpTarget::Column(column) => {
                if let Some(grid) = self.active_tab_mut().and_then(Tab::focused_grid_mut) {
                    grid.move_to_column(column);
                }
            }
        }
    }

    pub(crate) fn handle_raw_key(&mut self, chord: KeyChord) {
        if chord.code == KeyCode::Esc
            && !self.notifications.items().is_empty()
            && !self.is_text_input_focused()
        {
            self.notifications.dismiss_all();
        }
        match self.focus {
            Focus::Sidebar => self.explorer_raw_key(chord),
            Focus::Workspace => match self.active_tab() {
                Some(Tab::Editor(editor)) if editor.focus == EditorFocus::Text => {
                    self.editor_raw_key(chord)
                }
                Some(Tab::Editor(_)) => self.results_raw_key(chord),
                Some(Tab::Table(_)) => self.table_raw_key(chord),
                Some(Tab::KeyValue(_)) => self.key_value_raw_key(chord),
                None => {}
            },
        }
    }

    pub(crate) fn paste(&mut self, text: &str) {
        if let Some(overlay) = self.overlays.last_mut() {
            overlay_paste(overlay, text);
            return;
        }
        match (self.focus, self.workspace.active_tab_mut()) {
            (Focus::Sidebar, _) if self.explorer.filtering => self.explorer.filter.insert_str(text),
            (Focus::Workspace, Some(Tab::Editor(editor))) if editor.focus == EditorFocus::Text => {
                editor.buffer.insert_text(text);
                editor.completion = None;
            }
            (Focus::Workspace, Some(tab)) => paste_into_tab_input(tab, text),
            _ => {}
        }
    }

    pub(crate) fn handle_mouse(&mut self, mouse: MouseInput) {
        let Some(target) = self.hit_map.hit(mouse.column, mouse.row) else {
            return;
        };
        let delta = match mouse.kind {
            MouseKind::ScrollUp => -MOUSE_SCROLL_LINES,
            MouseKind::ScrollDown => MOUSE_SCROLL_LINES,
            MouseKind::Click => 0,
        };
        match target {
            HitTarget::ExplorerRow(row) => {
                self.focus = Focus::Sidebar;
                if delta == 0 {
                    self.explorer.selected = row;
                } else {
                    self.explorer.move_selection(delta);
                }
            }
            HitTarget::TabHeader(tab) => {
                self.workspace.activate_tab(tab);
                self.focus = Focus::Workspace;
            }
            HitTarget::Pane(pane) => {
                self.workspace.focus_pane(pane);
                self.focus = Focus::Workspace;
                if delta != 0 {
                    self.scroll_active(delta);
                }
            }
            HitTarget::GridRow { pane, row } => {
                self.workspace.focus_pane(pane);
                self.focus = Focus::Workspace;
                if let Some(grid) = self.active_tab_mut().and_then(Tab::focused_grid_mut) {
                    if delta == 0 {
                        grid.move_to_row(row);
                    } else {
                        grid.move_rows(delta);
                    }
                }
            }
            HitTarget::KeyRow { pane, row } => {
                self.workspace.focus_pane(pane);
                self.focus = Focus::Workspace;
                if delta == 0 {
                    self.select_key_row(row);
                }
            }
        }
    }

    fn scroll_active(&mut self, delta: isize) {
        match self.active_tab_mut() {
            Some(Tab::Editor(editor)) if editor.focus == EditorFocus::Text => {
                editor.scroll_line = editor.scroll_line.saturating_add_signed(delta);
            }
            Some(tab) => {
                if let Some(grid) = tab.focused_grid_mut() {
                    grid.move_rows(delta);
                }
            }
            None => {}
        }
    }

    pub(crate) fn request_quit(&mut self) {
        let pending = self
            .workspace
            .tabs()
            .any(|(_, tab)| tab.has_pending_changes());
        if pending {
            self.overlays.push(Overlay::Confirm(ConfirmState {
                title: "Quit RELIX?".into(),
                lines: vec![
                    "Some tables have uncommitted data changes that will be lost.".into(),
                    "Unsaved scripts are kept as drafts and restored next time.".into(),
                ],
                confirm_label: "Quit".into(),
                danger: true,
                action: PendingAction::Quit,
            }));
            return;
        }
        self.should_quit = true;
        self.notify(Level::Info, "saving session…");
    }
}

fn overlay_paste(overlay: &mut Overlay, text: &str) {
    match overlay {
        Overlay::Picker(picker) => {
            picker.input.insert_str(text);
            picker.refilter();
        }
        Overlay::Prompt(prompt) => prompt.input.insert_str(text),
        Overlay::TextEdit(edit) => edit.buffer.insert_text(text),
        Overlay::DataSourceForm(form) => {
            if let Some(field) = form.focused_field_mut()
                && let crate::datasource_form::FieldValue::Text(input)
                | crate::datasource_form::FieldValue::Secret(input) = &mut field.value
            {
                input.insert_str(text);
                if field.id == crate::datasource_form::FieldId::Password {
                    form.password_touched = true;
                }
            }
        }
        _ => {}
    }
}

fn paste_into_tab_input(tab: &mut Tab, text: &str) {
    match tab {
        Tab::Table(table) => {
            if let Some(editor) = table.grid.as_mut().and_then(|grid| grid.editing.as_mut()) {
                editor.input.insert_str(text);
            } else if table.focus == crate::table_tab::TableFocus::Filter {
                table.query.filter_input.insert_str(text);
            }
        }
        Tab::KeyValue(tab) => match tab.focus {
            crate::keyvalue_tab::KeyValueFocus::Pattern => tab.pattern.insert_str(text),
            crate::keyvalue_tab::KeyValueFocus::Console => tab.console_input.insert_str(text),
            _ => {}
        },
        Tab::Editor(_) => {}
    }
}
