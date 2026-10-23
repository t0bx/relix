/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use super::connections::Deferred;
use crate::buffer_keys::{BufferKey, apply_buffer_key};
use crate::command::Command;
use crate::editor_tab::{
    CompletionPopup, EditorFocus, EditorTab, ResultEntry, ResultKind, RunningQuery, SearchBar,
    SearchField,
};
use crate::events::AppEvent;
use crate::grid::{Editability, GridState};
use crate::input::LineInput;
use crate::notifications::Level;
use crate::overlay::{ConfirmState, Overlay, PendingAction, PromptAction};
use crate::result_query::ResultQuery;
use crate::tab::Tab;
use crate::workspace::TabId;
use chrono::Utc;
use relix_config::{KeyChord, KeyCode};
use relix_core::{DataSourceId, Dialect, StatementOutcome};
use relix_driver_api::ExecuteOptions;
use relix_editor::{CompletionCatalog, Diagnostic, SearchState, complete};
use relix_query::splitter::{split_statements, statement_at};
use relix_query::{
    HistoryEntry, ScriptOutcome, ScriptRequest, StatementKind, StatementReport, classify,
};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const EDITOR_PAGE_LINES: isize = 20;
const HISTORY_FRAGMENTS: usize = 50;

impl App {
    pub(crate) fn next_console_title(&self) -> String {
        let taken: Vec<String> = self
            .workspace
            .tabs()
            .filter_map(|(_, tab)| match tab {
                Tab::Editor(editor) => Some(editor.title.clone()),
                _ => None,
            })
            .collect();
        (1..)
            .map(|number| format!("console_{number}"))
            .find(|title| !taken.contains(title))
            .unwrap_or_else(|| "console".to_owned())
    }

    pub(crate) fn new_editor_tab(&mut self, source: Option<DataSourceId>, text: &str) -> TabId {
        let title = self.next_console_title();
        let pane = self.workspace.focused_pane();
        let id = self.workspace.add_tab(
            pane,
            Tab::Editor(Box::new(EditorTab::new(title, source, text))),
        );
        self.focus_workspace();
        id
    }

    fn active_editor(&mut self) -> Option<&mut EditorTab> {
        self.workspace.active_tab_mut().and_then(Tab::as_editor_mut)
    }

    pub(crate) fn editor_command(&mut self, command: Command) {
        match command {
            Command::Complete => self.update_completion(true),
            Command::Find | Command::Filter => self.open_search(false),
            Command::Replace => self.open_search(true),
            Command::FindNext => self.search_step(true),
            Command::FindPrevious => self.search_step(false),
            Command::Copy => {
                let text = self
                    .active_editor()
                    .and_then(|editor| editor.buffer.selected_text());
                if let Some(text) = text {
                    self.copy_to_clipboard(&text);
                }
            }
            Command::Cut => {
                let text = self
                    .active_editor()
                    .and_then(|editor| editor.buffer.cut_selection());
                if let Some(text) = text {
                    self.copy_to_clipboard(&text);
                }
            }
            Command::Paste => {
                if let Some(text) = self.context.clipboard.text() {
                    self.paste(&text);
                }
            }
            Command::ChooseDataSource => self.open_editor_data_source_picker(),
            Command::FocusResults => self.set_editor_focus(EditorFocus::Results),
            _ => {
                let Some(editor) = self.active_editor() else {
                    return;
                };
                match command {
                    Command::Undo => {
                        editor.buffer.undo();
                    }
                    Command::Redo => {
                        editor.buffer.redo();
                    }
                    Command::SelectAll => editor.buffer.select_all(),
                    Command::ToggleComment => editor.buffer.toggle_line_comment("--"),
                    Command::DuplicateLine => editor.buffer.duplicate_line(),
                    Command::DeleteLine => editor.buffer.delete_line(),
                    Command::MoveUp => editor.buffer.move_up(false),
                    Command::MoveDown => editor.buffer.move_down(false),
                    Command::PageUp => editor.buffer.move_vertical(-EDITOR_PAGE_LINES, false),
                    Command::PageDown => editor.buffer.move_vertical(EDITOR_PAGE_LINES, false),
                    Command::MoveTop => editor.buffer.move_document_start(false),
                    Command::MoveBottom => editor.buffer.move_document_end(false),
                    _ => {}
                }
                editor.completion = None;
            }
        }
    }

    pub(crate) fn copy_to_clipboard(&mut self, text: &str) {
        match self.context.clipboard.set_text(text) {
            Ok(()) => self.status = format!("copied {} characters", text.chars().count()),
            Err(error) => self.notify(Level::Warning, format!("clipboard unavailable: {error}")),
        }
    }

    /// Keys typed while the editor text has focus and no shortcut matched.
    pub(crate) fn editor_raw_key(&mut self, chord: KeyChord) {
        let tab_width = usize::from(self.settings.editor.tab_width);
        let auto_complete = self.settings.editor.auto_complete;
        let Some(editor) = self.active_editor() else {
            return;
        };
        if chord.code == KeyCode::Esc {
            editor.buffer.clear_selection();
            editor.diagnostics.clear();
            editor.completion = None;
            return;
        }
        let result = apply_buffer_key(&mut editor.buffer, chord, tab_width);
        if result == BufferKey::Ignored {
            return;
        }
        editor.completion = None;
        let trigger = matches!(result, BufferKey::Typed(ch) if ch.is_alphanumeric() || ch == '_' || ch == '.');
        if auto_complete && trigger {
            self.update_completion(false);
        }
    }

    /// Keys handled before the keymap while an in-editor popup (completion or search bar)
    /// is open. Returns whether the key was consumed.
    pub(crate) fn intercept_editor_key(&mut self, chord: KeyChord) -> bool {
        let Some(editor) = self.active_editor() else {
            return false;
        };
        if editor.focus != EditorFocus::Text {
            return false;
        }
        if editor.search.is_some() {
            return self.search_key(chord);
        }
        let Some(popup) = editor.completion.as_mut() else {
            return false;
        };
        let count = popup.items.len().max(1);
        match chord.code {
            KeyCode::Up => popup.selected = (popup.selected + count - 1) % count,
            KeyCode::Down => popup.selected = (popup.selected + 1) % count,
            KeyCode::PageUp => popup.selected = popup.selected.saturating_sub(8),
            KeyCode::PageDown => popup.selected = (popup.selected + 8).min(count - 1),
            KeyCode::Enter | KeyCode::Tab if !chord.modifiers.ctrl && !chord.modifiers.alt => {
                self.accept_completion()
            }
            KeyCode::Esc => editor.completion = None,
            _ => return false,
        }
        true
    }

    fn accept_completion(&mut self) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let Some(popup) = editor.completion.take() else {
            return;
        };
        let Some(item) = popup.items.get(popup.selected) else {
            return;
        };
        let cursor = editor.buffer.cursor();
        editor.buffer.select_range(popup.replace_start, cursor);
        editor.buffer.insert_text(&item.insert_text);
    }

    /// Recomputes completion for the cursor. Column metadata that the catalog lacks is
    /// requested in the background; the popup refreshes when it arrives.
    pub(crate) fn update_completion(&mut self, force: bool) {
        let Some(tab_id) = self.workspace.active_tab_id() else {
            return;
        };
        let Some(Tab::Editor(editor)) = self.workspace.tab(tab_id) else {
            return;
        };
        let source = editor.data_source;
        let dialect = source.map_or(Dialect::Generic, |source| self.dialect(source));
        let mut catalog = source
            .and_then(|source| self.catalogs.get(&source).cloned())
            .unwrap_or_else(|| CompletionCatalog::empty(dialect));
        catalog.fragments = self.history.fragments(HISTORY_FRAGMENTS);
        let text = editor.buffer.text();
        let offset = editor.buffer.offset_of(editor.buffer.cursor());
        let result = complete(&catalog, &text, offset);
        let replace_start = editor.buffer.position_of(result.replace_start);
        let typed_dot = text[..offset].ends_with('.');
        let show = !result.items.is_empty() && (force || !result.prefix.is_empty() || typed_dot);
        if let Some(source) = source {
            for (schema, table) in result.missing_tables {
                self.request_catalog_columns(source, schema, table);
            }
        }
        if let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab_id) {
            editor.completion = show.then_some(CompletionPopup {
                items: result.items,
                selected: 0,
                replace_start,
            });
        }
    }

    fn open_search(&mut self, replace: bool) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let initial = editor
            .buffer
            .selected_text()
            .filter(|text| !text.contains('\n'))
            .unwrap_or_default();
        let bar = editor.search.get_or_insert_with(|| SearchBar {
            state: SearchState::default(),
            query: LineInput::with_text(&initial),
            replacement: LineInput::default(),
            replace_mode: replace,
            field: SearchField::Query,
        });
        bar.replace_mode |= replace;
        bar.field = if replace && !bar.query.is_empty() {
            SearchField::Replacement
        } else {
            SearchField::Query
        };
        bar.state.query = bar.query.text().to_owned();
        bar.state.refresh(&editor.buffer);
    }

    fn search_step(&mut self, forward: bool) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let Some(bar) = editor.search.as_mut() else {
            return;
        };
        let found = if forward {
            bar.state.next(&mut editor.buffer)
        } else {
            bar.state.previous(&mut editor.buffer)
        };
        if found.is_none() {
            self.status = "no matches".into();
        }
    }

    fn search_key(&mut self, chord: KeyChord) -> bool {
        let Some(editor) = self.active_editor() else {
            return false;
        };
        let Some(bar) = editor.search.as_mut() else {
            return false;
        };
        match chord.code {
            KeyCode::Esc => {
                editor.search = None;
                return true;
            }
            KeyCode::Tab | KeyCode::BackTab if bar.replace_mode => {
                bar.field = match bar.field {
                    SearchField::Query => SearchField::Replacement,
                    SearchField::Replacement => SearchField::Query,
                };
                return true;
            }
            KeyCode::Char('a') if chord.modifiers.ctrl && bar.replace_mode => {
                bar.state.replacement = bar.replacement.text().to_owned();
                let count = bar.state.replace_all(&mut editor.buffer);
                self.status = format!("replaced {count} occurrences");
                return true;
            }
            KeyCode::Char('c') if chord.modifiers.alt => {
                bar.state.case_sensitive = !bar.state.case_sensitive;
                bar.state.refresh(&editor.buffer);
                return true;
            }
            KeyCode::Enter if bar.field == SearchField::Replacement => {
                bar.state.replacement = bar.replacement.text().to_owned();
                if !bar.state.replace_current(&mut editor.buffer) {
                    bar.state.next(&mut editor.buffer);
                }
                return true;
            }
            KeyCode::Enter | KeyCode::Down => {
                bar.state.next(&mut editor.buffer);
                return true;
            }
            KeyCode::Up => {
                bar.state.previous(&mut editor.buffer);
                return true;
            }
            _ => {}
        }
        if chord.modifiers.ctrl
            && !matches!(
                chord.code,
                KeyCode::Char('w' | 'u') | KeyCode::Backspace | KeyCode::Left | KeyCode::Right
            )
        {
            return false;
        }
        let field = bar.field;
        let input = match field {
            SearchField::Query => &mut bar.query,
            SearchField::Replacement => &mut bar.replacement,
        };
        if !super::explorer_actions::edit_line(input, chord) {
            return false;
        }
        if field == SearchField::Query {
            bar.state.query = bar.query.text().to_owned();
            let cursor = editor
                .buffer
                .selection()
                .map_or(editor.buffer.cursor(), |(start, _)| start);
            editor.buffer.set_cursor(cursor, false);
            bar.state.next(&mut editor.buffer);
        }
        true
    }

    /// Executes the selection, the statement under the cursor or the whole script. On
    /// production/staging data sources, destructive statements need confirmation first.
    pub(crate) fn run_editor(&mut self, script: bool, confirmed: bool) {
        let Some(tab_id) = self.workspace.active_tab_id() else {
            return;
        };
        let Some(Tab::Editor(editor)) = self.workspace.tab(tab_id) else {
            return;
        };
        if editor.running.is_some() {
            self.notify(
                Level::Info,
                "a query is already running in this tab (cancel with F8)",
            );
            return;
        }
        let Some(source) = editor.data_source else {
            self.open_editor_data_source_picker();
            self.notify(Level::Info, "choose a data source for this query tab");
            return;
        };
        if self
            .capabilities(source)
            .is_some_and(|capabilities| !capabilities.is_relational())
        {
            self.notify(
                Level::Info,
                "this data source uses the key browser and command console",
            );
            self.open_key_value_tab(source, false);
            return;
        }
        let dialect = self.dialect(source);
        let statements = editor_statements(editor, dialect, script);
        if statements.is_empty() {
            self.notify(Level::Info, "nothing to execute");
            return;
        }
        if !confirmed && self.requires_destructive_confirmation(source, &statements, dialect) {
            let name = self
                .data_source(source)
                .map(|definition| definition.name.clone())
                .unwrap_or_default();
            self.overlays.push(Overlay::Confirm(ConfirmState {
                title: "Run destructive statement?".into(),
                lines: std::iter::once(format!(
                    "Data source '{name}' is marked as a protected environment."
                ))
                .chain(
                    statements
                        .iter()
                        .filter(|(text, _)| classify(text, dialect).destructive)
                        .map(|(text, _)| {
                            format!(
                                "  {}",
                                relix_core::value::truncate_chars(&text.replace('\n', " "), 70)
                            )
                        }),
                )
                .collect(),
                confirm_label: "Execute".into(),
                danger: true,
                action: PendingAction::RunStatements {
                    tab: tab_id,
                    script,
                },
            }));
            return;
        }
        let Some(connection) = self.connection_or_defer(
            source,
            Deferred::RunEditor {
                tab: tab_id,
                script,
            },
        ) else {
            self.status = "connecting…".into();
            return;
        };
        let request = self.next_request();
        let cancel = CancellationToken::new();
        let row_limit = self.settings.query.row_limit;
        let timeout = (self.settings.query.timeout_secs > 0)
            .then(|| Duration::from_secs(self.settings.query.timeout_secs));
        let offsets: Vec<usize> = statements.iter().map(|(_, offset)| *offset).collect();
        let script_request = ScriptRequest {
            statements: statements.into_iter().map(|(text, _)| text).collect(),
            options: ExecuteOptions { row_limit },
            timeout,
        };
        if let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab_id) {
            editor.results.clear();
            editor.active_result = 0;
            editor.diagnostics.clear();
            editor.completion = None;
            editor.running = Some(RunningQuery {
                request,
                cancel: cancel.clone(),
                started: Instant::now(),
                total: script_request.statements.len(),
                completed: 0,
                statement_offsets: offsets,
            });
        }
        self.status = "executing…".into();
        let sender = self.event_sender();
        self.context.runtime.spawn(async move {
            let (report_sender, mut reports) = tokio::sync::mpsc::unbounded_channel();
            let run = relix_query::run_script(connection, script_request, cancel, move |report| {
                let _ = report_sender.send(report);
            });
            tokio::pin!(run);
            let outcome = loop {
                tokio::select! {
                    Some(report) = reports.recv() => {
                        let _ = sender.send(AppEvent::Statement { tab: tab_id, request, report }).await;
                    }
                    outcome = &mut run => break outcome,
                }
            };
            while let Ok(report) = reports.try_recv() {
                let _ = sender.send(AppEvent::Statement { tab: tab_id, request, report }).await;
            }
            let _ = sender.send(AppEvent::ScriptFinished { tab: tab_id, request, outcome }).await;
        });
    }

    fn requires_destructive_confirmation(
        &self,
        source: DataSourceId,
        statements: &[(String, usize)],
        dialect: Dialect,
    ) -> bool {
        let protected = self
            .data_source(source)
            .is_some_and(|definition| definition.environment.requires_confirmation());
        let enabled = self
            .project
            .as_ref()
            .and_then(|project| project.settings.confirm_destructive)
            .unwrap_or(self.settings.query.confirm_destructive);
        protected
            && enabled
            && statements
                .iter()
                .any(|(text, _)| classify(text, dialect).destructive)
    }

    pub(crate) fn on_statement_report(
        &mut self,
        tab: TabId,
        request: crate::requests::RequestId,
        report: StatementReport,
    ) {
        let row_limit = u32::try_from(self.settings.query.row_limit).unwrap_or(u32::MAX);
        let dialect = match self.workspace.tab(tab) {
            Some(Tab::Editor(editor)) => editor
                .data_source
                .map_or(Dialect::Generic, |source| self.dialect(source)),
            _ => return,
        };
        let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab) else {
            return;
        };
        let Some(running) = editor
            .running
            .as_mut()
            .filter(|running| running.request == request)
        else {
            return;
        };
        running.completed += 1;
        let offset = running
            .statement_offsets
            .get(report.index)
            .copied()
            .unwrap_or(0);
        let source = editor.data_source;
        let traits = classify(&report.statement, dialect);
        let (kind, summary, success) = match &report.result {
            Ok(StatementOutcome::Rows(batch)) => {
                let mut query = ResultQuery::new(row_limit);
                query.has_more = batch.has_more;
                let summary = format!(
                    "{}{} rows",
                    batch.rows.len(),
                    if batch.has_more { "+" } else { "" }
                );
                let grid = Box::new(GridState::from_batch(
                    batch,
                    Editability::ReadOnly(
                        "query results are read-only; open the table to edit".into(),
                    ),
                ));
                let wrappable = relix_query::wrapping::is_wrappable(&report.statement, dialect);
                (
                    ResultKind::Rows {
                        grid,
                        query,
                        wrappable,
                        loading: false,
                    },
                    summary,
                    true,
                )
            }
            Ok(StatementOutcome::Affected { rows }) => (
                ResultKind::Affected(*rows),
                format!("{rows} rows affected"),
                true,
            ),
            Ok(StatementOutcome::Completed { message }) => {
                (ResultKind::Message(message.clone()), message.clone(), true)
            }
            Err(error) => {
                let diagnostic = Diagnostic::from_server_error(
                    &editor.buffer,
                    offset,
                    error.position,
                    &error.message,
                );
                editor.diagnostics.push(diagnostic);
                (ResultKind::Error(error.clone()), error.to_string(), false)
            }
        };
        if success {
            match traits.kind {
                StatementKind::TransactionStart => editor.in_transaction = true,
                StatementKind::TransactionEnd => editor.in_transaction = false,
                _ => {}
            }
        }
        editor.results.push(ResultEntry {
            statement: report.statement.clone(),
            statement_offset: offset,
            kind,
            elapsed: report.elapsed,
            request: None,
        });
        self.status = format!("{summary} · {} ms", report.elapsed.as_millis());
        self.record_history(HistoryEntry {
            sql: report.statement,
            data_source: source,
            executed_at: Utc::now(),
            duration_ms: u64::try_from(report.elapsed.as_millis()).unwrap_or(u64::MAX),
            success,
            summary,
        });
    }

    pub(crate) fn on_script_finished(
        &mut self,
        tab: TabId,
        request: crate::requests::RequestId,
        outcome: ScriptOutcome,
    ) {
        let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab) else {
            return;
        };
        if editor
            .running
            .as_ref()
            .is_none_or(|running| running.request != request)
        {
            return;
        }
        let started = editor.running.take().map(|running| running.started);
        let focus_index = editor
            .results
            .iter()
            .rposition(|result| matches!(result.kind, ResultKind::Error(_)))
            .or_else(|| {
                editor
                    .results
                    .iter()
                    .rposition(|result| matches!(result.kind, ResultKind::Rows { .. }))
            })
            .unwrap_or(editor.results.len().saturating_sub(1));
        editor.active_result = focus_index;
        let total = started
            .map(|started| started.elapsed().as_millis())
            .unwrap_or_default();
        match outcome {
            ScriptOutcome::Completed => {
                if editor.results.len() > 1 {
                    self.status =
                        format!("{} statements executed · {total} ms", editor.results.len());
                }
            }
            ScriptOutcome::Failed { index } => {
                self.notify(
                    Level::Error,
                    format!("statement {} failed; execution stopped", index + 1),
                );
            }
            ScriptOutcome::Cancelled { index } => {
                self.notify(
                    Level::Warning,
                    format!("execution cancelled at statement {}", index + 1),
                );
            }
        }
    }

    pub(crate) fn cancel_running_query(&mut self) {
        match self.active_editor() {
            Some(editor) => match &editor.running {
                Some(running) => {
                    running.cancel.cancel();
                    self.status = "cancelling…".into();
                }
                None => self.status = "no running query".into(),
            },
            None => self.status = "no running query".into(),
        }
    }

    fn record_history(&mut self, entry: HistoryEntry) {
        if let Some(project) = &self.project
            && let Err(error) = self.context.store.append_history(project.id, &entry)
        {
            tracing::warn!(%error, "failed to persist query history");
        }
        self.history.record(entry);
    }

    pub(crate) fn save_current(&mut self) {
        match self.active_tab() {
            Some(Tab::Editor(editor)) => match editor.path.clone() {
                Some(path) => {
                    let tab = self.workspace.active_tab_id();
                    self.write_editor_to(tab, &path);
                }
                None => self.prompt_save_as(),
            },
            Some(Tab::Table(_)) => self.review_changes(),
            _ => {}
        }
    }

    pub(crate) fn prompt_save_as(&mut self) {
        let Some(tab) = self.workspace.active_tab_id() else {
            return;
        };
        let Some(Tab::Editor(editor)) = self.workspace.tab(tab) else {
            return;
        };
        let suggestion = editor
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map_or_else(
                || format!("{}.sql", editor.title),
                |name| name.to_string_lossy().into_owned(),
            );
        self.open_prompt(
            "Save script as",
            "a name (saved in the project's scripts) or a path",
            PromptAction::SaveScriptAs(tab),
            &suggestion,
        );
    }

    pub(crate) fn save_script_as(&mut self, tab: TabId, name: &str) {
        let Some(project) = self.project.as_ref().map(|project| project.id) else {
            return;
        };
        let looks_like_path = name.contains(std::path::MAIN_SEPARATOR)
            || name.contains('/')
            || std::path::Path::new(name).is_absolute();
        let path = if looks_like_path {
            Ok(std::path::PathBuf::from(name))
        } else {
            self.context.store.script_path(project, name)
        };
        match path {
            Ok(path) => self.write_editor_to(Some(tab), &path),
            Err(error) => self.notify(Level::Error, error.to_string()),
        }
    }

    fn write_editor_to(&mut self, tab: Option<TabId>, path: &std::path::Path) {
        let Some(tab) = tab else {
            return;
        };
        let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab) else {
            return;
        };
        let text = editor.buffer.text();
        match relix_config::files::write_atomic(path, text.as_bytes()) {
            Ok(()) => {
                editor.buffer.mark_saved();
                editor.path = Some(path.to_path_buf());
                if let Some(name) = path.file_stem() {
                    editor.title = name.to_string_lossy().into_owned();
                }
                self.status = format!("saved {}", path.display());
            }
            Err(error) => self.notify(Level::Error, format!("could not save: {error}")),
        }
    }

    pub(crate) fn open_sql_file(&mut self, path: &str) {
        let path = std::path::PathBuf::from(path.trim());
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let source = self.active_tab().and_then(Tab::data_source);
                let tab = self.new_editor_tab(source, &text);
                if let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab) {
                    editor.buffer.mark_saved();
                    editor.title = path.file_stem().map_or_else(
                        || "script".to_owned(),
                        |stem| stem.to_string_lossy().into_owned(),
                    );
                    editor.path = Some(path);
                }
            }
            Err(error) => self.notify(
                Level::Error,
                format!("cannot open {}: {error}", path.display()),
            ),
        }
    }

    pub(crate) fn insert_into_editor(&mut self, text: &str) {
        let target = match self.active_tab() {
            Some(Tab::Editor(_)) => self.workspace.active_tab_id(),
            _ => None,
        };
        let tab = target.unwrap_or_else(|| self.new_editor_tab(None, ""));
        if let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab) {
            editor.buffer.insert_text(text);
            editor.focus = EditorFocus::Text;
        }
        self.focus_workspace();
    }
}

/// The statements to run with their byte offsets in the buffer: the selection when there
/// is one, every statement for "run script", otherwise the statement under the cursor.
fn editor_statements(editor: &EditorTab, dialect: Dialect, script: bool) -> Vec<(String, usize)> {
    let text = editor.buffer.text();
    if let Some((start, end)) = editor.buffer.selection() {
        let from = editor.buffer.offset_of(start);
        let to = editor.buffer.offset_of(end);
        let selected = &text[from..to];
        return split_statements(selected, dialect)
            .into_iter()
            .map(|statement| (statement.text, from + statement.start))
            .collect();
    }
    if script {
        return split_statements(&text, dialect)
            .into_iter()
            .map(|statement| (statement.text, statement.start))
            .collect();
    }
    let offset = editor.buffer.offset_of(editor.buffer.cursor());
    statement_at(&text, dialect, offset)
        .map(|statement| vec![(statement.text, statement.start)])
        .unwrap_or_default()
}
