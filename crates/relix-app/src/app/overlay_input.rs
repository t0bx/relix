/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use super::explorer_actions::edit_line;
use crate::buffer_keys::apply_buffer_key;
use crate::form::{FormCommand, FormStatus};
use crate::notifications::Level;
use crate::overlay::{
    ColumnChooserState, ConfirmState, JumpState, Overlay, PendingAction, PickerPurpose,
    PickerState, PromptAction, PromptState, SortEditorState, TextEditState, ViewerState,
};
use crate::tab::Tab;
use relix_config::{KeyChord, KeyCode};

const VIEWER_PAGE: usize = 15;

impl App {
    pub(crate) fn handle_overlay_key(&mut self, chord: KeyChord) {
        let Some(overlay) = self.overlays.pop() else {
            return;
        };
        let keep = match overlay {
            Overlay::Picker(picker) => self.picker_key(picker, chord),
            Overlay::DataSourceForm(form) => self.form_key(form, chord),
            Overlay::ServerManager(state) => self.server_manager_key(state, chord),
            Overlay::SshForm(form) => self.ssh_form_key(form, chord),
            Overlay::VpnForm(form) => self.vpn_form_key(form, chord),
            Overlay::Confirm(confirm) => self.confirm_key(confirm, chord),
            Overlay::Prompt(prompt) => self.prompt_key(prompt, chord),
            Overlay::Viewer(viewer) => self.viewer_key(viewer, chord),
            Overlay::TextEdit(edit) => self.text_edit_key(*edit, chord),
            Overlay::SortEditor(editor) => self.sort_editor_key(editor, chord),
            Overlay::ColumnChooser(chooser) => self.column_chooser_key(chooser, chord),
            Overlay::Jump(jump) => self.jump_key(jump, chord),
            Overlay::Help { scroll } => help_key(scroll, chord),
        };
        if let Some(overlay) = keep {
            self.overlays.push(overlay);
        }
    }

    fn picker_key(&mut self, mut picker: PickerState, chord: KeyChord) -> Option<Overlay> {
        let ctrl = chord.modifiers.ctrl;
        match chord.code {
            KeyCode::Esc => return None,
            KeyCode::Enter => {
                self.accept_picker(picker, false);
                return None;
            }
            KeyCode::Up => picker.move_selection(-1),
            KeyCode::Down | KeyCode::Tab => picker.move_selection(1),
            KeyCode::BackTab => picker.move_selection(-1),
            KeyCode::PageUp => picker.move_selection(-10),
            KeyCode::PageDown => picker.move_selection(10),
            KeyCode::Char('p' | 'k') if ctrl => picker.move_selection(-1),
            KeyCode::Char('j') if ctrl => picker.move_selection(1),
            KeyCode::Char('r') if ctrl && picker.purpose == PickerPurpose::History => {
                self.accept_picker(picker, true);
                return None;
            }
            KeyCode::Char(_) if ctrl && picker.purpose == PickerPurpose::Projects => {
                self.project_picker_shortcut(&picker, chord);
                return None;
            }
            KeyCode::Char(_) if ctrl && picker.purpose == PickerPurpose::Drivers => {
                return self.driver_picker_shortcut(picker, chord);
            }
            _ => {
                if edit_line(&mut picker.input, chord) {
                    picker.selected = 0;
                    picker.refilter();
                }
            }
        }
        Some(Overlay::Picker(picker))
    }

    fn form_key(
        &mut self,
        mut form: Box<crate::datasource_form::DataSourceForm>,
        chord: KeyChord,
    ) -> Option<Overlay> {
        match form.handle_key(chord) {
            FormCommand::Cancel => return None,
            FormCommand::Test => match form.build() {
                Ok((definition, password)) => {
                    form.set_status(FormStatus::success("testing connection…"), true);
                    self.test_connection(definition, password);
                }
                Err(error) => form.set_status(FormStatus::failure(error), false),
            },
            FormCommand::Submit => match form.build() {
                Ok((definition, password)) => {
                    self.save_data_source(definition, password);
                    return None;
                }
                Err(error) => form.set_status(FormStatus::failure(error), false),
            },
            FormCommand::Shortcut(_) | FormCommand::Edited(_) | FormCommand::Ignored => {}
        }
        Some(Overlay::DataSourceForm(form))
    }

    fn confirm_key(&mut self, confirm: ConfirmState, chord: KeyChord) -> Option<Overlay> {
        match chord.code {
            KeyCode::Enter | KeyCode::Char('y' | 'Y') => {
                self.execute_pending(confirm.action);
                None
            }
            KeyCode::Esc | KeyCode::Char('n' | 'N' | 'q') => None,
            _ => Some(Overlay::Confirm(confirm)),
        }
    }

    fn prompt_key(&mut self, mut prompt: PromptState, chord: KeyChord) -> Option<Overlay> {
        match chord.code {
            KeyCode::Esc => None,
            KeyCode::Enter => {
                let text = prompt.input.text().to_owned();
                self.submit_prompt(prompt.action, text);
                None
            }
            _ => {
                edit_line(&mut prompt.input, chord);
                Some(Overlay::Prompt(prompt))
            }
        }
    }

    fn viewer_key(&mut self, mut viewer: ViewerState, chord: KeyChord) -> Option<Overlay> {
        let last = viewer.lines.len().saturating_sub(1);
        match chord.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => return None,
            KeyCode::Up | KeyCode::Char('k') => viewer.scroll = viewer.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => viewer.scroll = (viewer.scroll + 1).min(last),
            KeyCode::PageUp => viewer.scroll = viewer.scroll.saturating_sub(VIEWER_PAGE),
            KeyCode::PageDown => viewer.scroll = (viewer.scroll + VIEWER_PAGE).min(last),
            KeyCode::Home | KeyCode::Char('g') => viewer.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => viewer.scroll = last,
            KeyCode::Char('y') => {
                let text = viewer.copy_text.clone();
                self.copy_to_clipboard(&text);
            }
            _ => {}
        }
        Some(Overlay::Viewer(viewer))
    }

    fn text_edit_key(&mut self, mut edit: TextEditState, chord: KeyChord) -> Option<Overlay> {
        match chord.code {
            KeyCode::Esc => None,
            KeyCode::Char('s') if chord.modifiers.ctrl => {
                let text = edit.buffer.text();
                self.save_text_edit(edit.action, text);
                None
            }
            KeyCode::Char('z') if chord.modifiers.ctrl => {
                edit.buffer.undo();
                Some(Overlay::TextEdit(Box::new(edit)))
            }
            _ => {
                apply_buffer_key(
                    &mut edit.buffer,
                    chord,
                    usize::from(self.settings.editor.tab_width),
                );
                Some(Overlay::TextEdit(Box::new(edit)))
            }
        }
    }

    fn sort_editor_key(&mut self, mut editor: SortEditorState, chord: KeyChord) -> Option<Overlay> {
        let tab = editor.tab;
        let Some(Tab::Table(table)) = self.workspace.tab_mut(tab) else {
            return None;
        };
        let count = table.query.sort.len();
        let mut changed = false;
        let shift = chord.modifiers.shift;
        match chord.code {
            KeyCode::Esc | KeyCode::Char('q') => return None,
            KeyCode::Char('K') | KeyCode::Up if shift || chord.code == KeyCode::Char('K') => {
                editor.selected = table.query.move_sort(editor.selected, true);
                changed = true;
            }
            KeyCode::Char('J') | KeyCode::Down if shift || chord.code == KeyCode::Char('J') => {
                editor.selected = table.query.move_sort(editor.selected, false);
                changed = true;
            }
            KeyCode::Up | KeyCode::Char('k') => editor.selected = editor.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                editor.selected = (editor.selected + 1).min(count.saturating_sub(1))
            }
            KeyCode::Char(' ') | KeyCode::Enter if count > 0 => {
                table.query.toggle_sort_direction(editor.selected);
                changed = true;
            }
            KeyCode::Char('d') | KeyCode::Delete | KeyCode::Backspace if count > 0 => {
                table.query.remove_sort(editor.selected);
                editor.selected = editor
                    .selected
                    .min(table.query.sort.len().saturating_sub(1));
                changed = true;
            }
            KeyCode::Enter => return None,
            _ => {}
        }
        if changed {
            self.sort_editor_changed(tab);
        }
        Some(Overlay::SortEditor(editor))
    }

    fn column_chooser_key(
        &mut self,
        mut chooser: ColumnChooserState,
        chord: KeyChord,
    ) -> Option<Overlay> {
        let grid = self
            .workspace
            .tab_mut(chooser.tab)
            .and_then(Tab::focused_grid_mut)?;
        let count = grid.columns.len();
        match chord.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => return None,
            KeyCode::Up | KeyCode::Char('k') => {
                chooser.selected = chooser.selected.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                chooser.selected = (chooser.selected + 1).min(count.saturating_sub(1))
            }
            KeyCode::Char(' ') | KeyCode::Char('x') => {
                let hidden = grid
                    .columns
                    .get(chooser.selected)
                    .is_some_and(|column| column.hidden);
                grid.set_column_hidden(chooser.selected, !hidden);
            }
            KeyCode::Char('a') => {
                for index in 0..count {
                    grid.set_column_hidden(index, false);
                }
            }
            _ => {}
        }
        Some(Overlay::ColumnChooser(chooser))
    }

    fn jump_key(&mut self, mut jump: JumpState, chord: KeyChord) -> Option<Overlay> {
        let KeyCode::Char(ch) = chord.code else {
            return None;
        };
        jump.typed.push(ch);
        if let Some((_, target)) = jump.targets.iter().find(|(label, _)| *label == jump.typed) {
            self.perform_jump(target.clone());
            return None;
        }
        let possible = jump
            .targets
            .iter()
            .any(|(label, _)| label.starts_with(&jump.typed));
        possible.then_some(Overlay::Jump(jump))
    }

    pub(crate) fn execute_pending(&mut self, action: PendingAction) {
        match action {
            PendingAction::DeleteProject(id) => self.delete_project(id),
            PendingAction::DeleteDataSource(id) => self.delete_data_source(id),
            PendingAction::CloseTab(tab) => self.close_tab(tab, true),
            PendingAction::ClosePane(pane) => {
                self.workspace.focus_pane(pane);
                self.close_focused_pane(true);
            }
            PendingAction::CommitChanges(tab) => self.commit_changes(tab),
            PendingAction::RunStatements { tab, script } => {
                self.workspace.activate_tab(tab);
                self.run_editor(script, true);
            }
            PendingAction::DeleteKey { tab, key } => self.delete_key(tab, key),
            PendingAction::DeleteElement { tab } => self.delete_element(tab),
            PendingAction::InstallDriver { offer, retry } => self.install_driver(offer, retry),
            PendingAction::TrustHostKey { prompt, retry } => self.trust_host_key(&prompt, retry),
            PendingAction::ApproveVpn { profile, retry } => self.approve_vpn(profile, retry),
            PendingAction::ConnectVpn(profile) => self.approve_and_connect_vpn(profile),
            PendingAction::DeleteDocument { tab, key } => self.delete_document(tab, key),
            PendingAction::DropDocumentIndex { tab, name } => self.drop_document_index(tab, name),
            PendingAction::DeleteServerConfig(id) => self.delete_server_config(id),
            PendingAction::TransferServerConfig { id, mode } => {
                self.transfer_server_config(id, mode)
            }
            PendingAction::RemoveDriver(id) => self.remove_driver(&id),
            PendingAction::SwitchProject(id) => self.switch_project(id),
            PendingAction::Quit => {
                self.should_quit = true;
            }
        }
    }

    fn submit_prompt(&mut self, action: PromptAction, text: String) {
        let trimmed = text.trim().to_owned();
        match action {
            PromptAction::NewProject => self.create_project(&trimmed),
            PromptAction::RenameProject(id) => self.rename_project(id, &trimmed),
            PromptAction::SaveScriptAs(tab) if !trimmed.is_empty() => {
                self.save_script_as(tab, &trimmed)
            }
            PromptAction::OpenFile if !trimmed.is_empty() => self.open_sql_file(&trimmed),
            PromptAction::ExportResults { tab, format } if !trimmed.is_empty() => {
                self.export_grid(tab, format, &trimmed)
            }
            PromptAction::ExportProject if !trimmed.is_empty() => self.export_project(&trimmed),
            PromptAction::ImportProject if !trimmed.is_empty() => self.import_project(&trimmed),
            PromptAction::SetTtl { tab, key } => self.set_ttl(tab, key, &trimmed),
            PromptAction::NewKey { tab } => self.create_key(tab, &text),
            PromptAction::AddElement { tab } => self.add_element(tab, &text),
            PromptAction::Password { source } => self.submit_password(source, text),
            PromptAction::RenameServerConfig(id) if !trimmed.is_empty() => {
                self.rename_server_config(id, &trimmed)
            }
            PromptAction::ServerSecret {
                server,
                kind,
                retry,
            } => self.submit_server_secret(server, kind, text, retry),
            PromptAction::ResultFilter { tab } => self.apply_result_filter(tab, &trimmed),
            _ => self.notify(Level::Info, "nothing entered"),
        }
    }
}

fn help_key(scroll: usize, chord: KeyChord) -> Option<Overlay> {
    match chord.code {
        KeyCode::Esc | KeyCode::Char('q' | '?') | KeyCode::F(1) | KeyCode::Enter => None,
        KeyCode::Up | KeyCode::Char('k') => Some(Overlay::Help {
            scroll: scroll.saturating_sub(1),
        }),
        KeyCode::Down | KeyCode::Char('j') => Some(Overlay::Help { scroll: scroll + 1 }),
        KeyCode::PageUp => Some(Overlay::Help {
            scroll: scroll.saturating_sub(VIEWER_PAGE),
        }),
        KeyCode::PageDown => Some(Overlay::Help {
            scroll: scroll + VIEWER_PAGE,
        }),
        _ => Some(Overlay::Help { scroll }),
    }
}
