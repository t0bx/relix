/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use super::connections::Deferred;
use super::explorer_actions::edit_line;
use crate::command::Command;
use crate::events::AppEvent;
use crate::grid::{Editability, GridState};
use crate::loaders;
use crate::notifications::Level;
use crate::overlay::{
    ConfirmState, Overlay, PendingAction, PickerItem, PickerPurpose, PickerState, PickerValue,
    SortEditorState, TextEditAction, TextEditState,
};
use crate::requests::RequestId;
use crate::tab::Tab;
use crate::table_tab::{FilterCompletion, TableFocus, TableTab};
use crate::workspace::TabId;
use relix_config::{KeyChord, KeyCode};
use relix_core::{
    ChangeReport, DataSourceId, ObjectKind, RowBatch, TableDetails, TableQuery, TableRef, Value,
};
use relix_driver_api::DriverError;
use relix_editor::{CatalogTable, CompletionCatalog, CompletionItem, CompletionKind, complete};
use relix_project::SplitDirection;
use relix_query::RecentList;
use relix_safety::Operation;

const FILTER_HISTORY_SIZE: usize = 30;
const VALUE_SUGGESTIONS: usize = 20;
const MAX_REVIEW_LINES: usize = 40;

impl App {
    pub(crate) fn open_table(&mut self, source: DataSourceId, table: TableRef, new_pane: bool) {
        if new_pane {
            self.workspace.split(SplitDirection::Vertical);
        } else if let Some(existing) = self.find_table_tab(source, &table) {
            self.workspace.activate_tab(existing);
            self.focus_workspace();
            return;
        }
        let tab = TableTab::new(source, table, self.settings.query.page_size);
        let pane = self.workspace.focused_pane();
        let id = self.workspace.add_tab(pane, Tab::Table(Box::new(tab)));
        self.focus_workspace();
        self.load_table(id);
    }

    fn find_table_tab(&self, source: DataSourceId, table: &TableRef) -> Option<TabId> {
        let pane = self.workspace.pane(self.workspace.focused_pane())?;
        pane.tabs.iter().copied().find(|id| {
            matches!(self.workspace.tab(*id), Some(Tab::Table(tab)) if tab.data_source == source && &tab.table == table)
        })
    }

    fn table_tab(&mut self, tab: TabId) -> Option<&mut TableTab> {
        self.workspace.tab_mut(tab).and_then(Tab::as_table_mut)
    }

    pub(crate) fn load_table(&mut self, tab: TabId) {
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        let source = table.data_source;
        let has_details = table.details.is_some();
        let reference = table.table.clone();
        table.loading = true;
        table.error = None;
        let Some(connection) = self.connection_or_defer(source, Deferred::LoadTable(tab)) else {
            return;
        };
        if has_details {
            self.fetch_table_page(tab);
            return;
        }
        self.spawn(async move {
            let result = match loaders::sql(connection.as_ref()) {
                Ok(sql) => sql.table_details(&reference).await,
                Err(error) => Err(error),
            };
            AppEvent::TableDetails { tab, result }
        });
    }

    pub(crate) fn on_table_details(
        &mut self,
        tab: TabId,
        result: Result<TableDetails, DriverError>,
    ) {
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        match result {
            Ok(details) => {
                let source = table.data_source;
                let columns = loaders::catalog_columns(&details);
                let schema = details
                    .table
                    .schema
                    .clone()
                    .or_else(|| details.table.database.clone());
                let name = details.table.name.clone();
                table.details = Some(details);
                if let Some(catalog) = self.catalogs.get_mut(&source) {
                    catalog.set_columns(schema.as_deref(), &name, columns);
                }
                self.fetch_table_page(tab);
            }
            Err(error) => {
                table.loading = false;
                table.error = Some(error.to_string());
                self.notify(Level::Error, error.to_string());
            }
        }
    }

    fn fetch_table_page(&mut self, tab: TabId) {
        let request = self.next_request();
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        let source = table.data_source;
        let query = TableQuery {
            filter: table.query.applied_filter.clone(),
            sort: table.query.sort.clone(),
            tiebreaker: table
                .details
                .as_ref()
                .and_then(TableDetails::row_identity)
                .unwrap_or_default(),
            offset: table.query.offset,
            limit: table.query.page_size,
        };
        let reference = table.table.clone();
        table.loading = true;
        table.request = Some(request);
        let Some(connection) = self.connection(source) else {
            self.load_table(tab);
            return;
        };
        self.spawn(async move {
            let result = match loaders::sql(connection.as_ref()) {
                Ok(sql) => sql.fetch_table_page(&reference, &query).await,
                Err(error) => Err(error),
            };
            AppEvent::TablePage {
                tab,
                request,
                result,
            }
        });
    }

    pub(crate) fn on_table_page(
        &mut self,
        tab: TabId,
        request: RequestId,
        result: Result<RowBatch, DriverError>,
    ) {
        let read_only = self
            .workspace
            .tab(tab)
            .and_then(Tab::data_source)
            .is_some_and(|source| self.is_read_only(source));
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        if table.request != Some(request) {
            return;
        }
        table.loading = false;
        table.request = None;
        match result {
            Ok(batch) => {
                table.query.has_more = batch.has_more;
                table.error = None;
                match table.grid.as_mut() {
                    Some(grid) => grid.replace_rows(&batch),
                    None => {
                        let mut grid = GridState::from_batch(
                            &batch,
                            table_editability(table.details.as_ref(), read_only),
                        );
                        grid.hide_columns_named(&table.restore_hidden);
                        grid.view = table.restore_view;
                        table.grid = Some(grid);
                    }
                }
                let label = table.query.page_label(batch.rows.len());
                self.status = format!("{} · rows {label}", table.table);
            }
            Err(error) => {
                table.error = Some(error.to_string());
                let source = table.data_source;
                self.notify(Level::Error, error.to_string());
                self.note_driver_error(source, &error);
            }
        }
    }

    pub(crate) fn reload_table_page(&mut self) {
        let Some(tab) = self.workspace.active_tab_id() else {
            return;
        };
        let pending = self.table_tab(tab).is_some_and(|table| {
            table
                .grid
                .as_ref()
                .is_some_and(GridState::has_pending_changes)
        });
        if pending {
            self.notify(
                Level::Warning,
                "commit (Ctrl+S) or discard (U) pending changes before reloading",
            );
            return;
        }
        if let Some(table) = self.table_tab(tab) {
            table.query.total_rows = None;
        }
        self.load_table(tab);
    }

    pub(crate) fn table_command(&mut self, command: Command) {
        let Some(tab) = self.workspace.active_tab_id() else {
            return;
        };
        if self.grid_command(command) {
            return;
        }
        match command {
            Command::Filter => {
                if let Some(table) = self.table_tab(tab) {
                    table.focus = TableFocus::Filter;
                    table.query.filter_input.move_end();
                }
            }
            Command::Refresh => self.reload_table_page(),
            Command::NextPage | Command::PreviousPage => {
                self.change_table_page(tab, command == Command::NextPage)
            }
            Command::SortColumn | Command::AddSortColumn => {
                self.sort_table(tab, command == Command::AddSortColumn)
            }
            Command::EditSort => self
                .overlays
                .push(Overlay::SortEditor(SortEditorState { tab, selected: 0 })),
            Command::CountRows => self.count_table_rows(tab),
            Command::ReviewChanges => self.review_changes(),
            Command::DiscardChanges => {
                if let Some(grid) = self.table_tab(tab).and_then(|table| table.grid.as_mut()) {
                    grid.discard_changes();
                    self.status = "pending changes discarded".into();
                }
            }
            Command::EditCell
            | Command::SetNull
            | Command::InsertRow
            | Command::DeleteRows
            | Command::RevertChange => {
                self.edit_table_grid(tab, command);
            }
            Command::EditCellInPopup => self.edit_cell_in_popup(tab),
            _ => {}
        }
    }

    fn edit_table_grid(&mut self, tab: TabId, command: Command) {
        let Some(grid) = self.table_tab(tab).and_then(|table| table.grid.as_mut()) else {
            return;
        };
        let result = match command {
            Command::EditCell => grid.begin_edit(),
            Command::SetNull => grid.set_null(),
            Command::InsertRow => grid.insert_row(),
            Command::DeleteRows => grid.delete_target_rows().map(|count| {
                self.status =
                    format!("{count} row(s) marked for deletion — Ctrl+S to review & commit");
            }),
            Command::RevertChange => {
                grid.revert_current();
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(reason) = result {
            self.notify(Level::Warning, reason);
        }
    }

    fn edit_cell_in_popup(&mut self, tab: TabId) {
        let Some(grid) = self.table_tab(tab).and_then(|table| table.grid.as_ref()) else {
            return;
        };
        if let Editability::ReadOnly(reason) = &grid.editability {
            let reason = reason.clone();
            self.notify(Level::Warning, reason);
            return;
        }
        let (row, column) = (grid.cursor_row, grid.cursor_column);
        let text = grid
            .current_value()
            .and_then(Value::to_plain_string)
            .unwrap_or_default();
        let title = grid.current_column().map_or_else(
            || "cell".to_owned(),
            |column| format!("Edit {}", column.meta.name),
        );
        self.overlays
            .push(Overlay::TextEdit(Box::new(TextEditState::new(
                title,
                &text,
                TextEditAction::GridCell { tab, row, column },
            ))));
    }

    pub(crate) fn stage_cell_text(&mut self, tab: TabId, row: usize, column: usize, text: String) {
        let Some(grid) = self.table_tab(tab).and_then(|table| table.grid.as_mut()) else {
            return;
        };
        if let Err(reason) = grid.set_cell(row, column, Value::Text(text)) {
            self.notify(Level::Warning, reason);
        }
    }

    fn change_table_page(&mut self, tab: TabId, forward: bool) {
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        if table
            .grid
            .as_ref()
            .is_some_and(GridState::has_pending_changes)
        {
            self.notify(
                Level::Warning,
                "commit or discard pending changes before paging",
            );
            return;
        }
        let moved = if forward {
            table.query.next_page()
        } else {
            table.query.previous_page()
        };
        if moved {
            self.fetch_table_page(tab);
        }
    }

    fn sort_table(&mut self, tab: TabId, multi: bool) {
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        let Some(column) = table
            .grid
            .as_ref()
            .and_then(|grid| grid.current_column())
            .map(|column| column.meta.name.clone())
        else {
            return;
        };
        if table
            .grid
            .as_ref()
            .is_some_and(GridState::has_pending_changes)
        {
            self.notify(
                Level::Warning,
                "commit or discard pending changes before sorting",
            );
            return;
        }
        if multi {
            table.query.add_sort(&column);
        } else {
            table.query.cycle_sort(&column);
        }
        self.fetch_table_page(tab);
    }

    pub(crate) fn apply_table_sort(&mut self, tab: TabId) {
        self.fetch_table_page(tab);
    }

    fn count_table_rows(&mut self, tab: TabId) {
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        let (source, reference, filter) = (
            table.data_source,
            table.table.clone(),
            table.query.applied_filter.clone(),
        );
        let Some(connection) = self.connection(source) else {
            return;
        };
        self.status = "counting rows…".into();
        self.spawn(async move {
            let result = match loaders::sql(connection.as_ref()) {
                Ok(sql) => sql.count_rows(&reference, filter.as_deref()).await,
                Err(error) => Err(error),
            };
            AppEvent::RowCount { tab, result }
        });
    }

    pub(crate) fn on_row_count(&mut self, tab: TabId, result: Result<u64, DriverError>) {
        match result {
            Ok(count) => {
                if let Some(table) = self.table_tab(tab) {
                    table.query.total_rows = Some(count);
                }
                self.status = format!("{count} rows");
            }
            Err(error) => self.notify(Level::Error, error.to_string()),
        }
    }

    /// Shows every staged modification for review; nothing reaches the database until the
    /// user confirms, and then all changes run in one transaction.
    pub(crate) fn review_changes(&mut self) {
        let Some(tab) = self.workspace.active_tab_id() else {
            return;
        };
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        let Some(grid) = table.grid.as_ref() else {
            return;
        };
        if grid.editing.is_some()
            && let Some(grid) = table.grid.as_mut()
        {
            grid.commit_edit();
        }
        let Some(grid) = table.grid.as_ref() else {
            return;
        };
        if !grid.has_pending_changes() {
            self.status = "no pending changes".into();
            return;
        }
        let source = table.data_source;
        let name = table.table.to_string();
        let mut lines: Vec<String> = grid.change_summary();
        let total = lines.len();
        lines.truncate(MAX_REVIEW_LINES);
        if total > MAX_REVIEW_LINES {
            lines.push(format!("… and {} more", total - MAX_REVIEW_LINES));
        }
        lines.insert(
            0,
            format!("{total} change(s) to {name}, applied in one transaction:"),
        );
        lines.insert(1, String::new());
        let danger = self
            .data_source(source)
            .is_some_and(|definition| definition.environment.requires_confirmation());
        self.confirm_write(
            source,
            &Operation::TableChanges {
                table: &name,
                changes: total,
            },
            ConfirmState {
                title: "Review pending changes".into(),
                lines,
                confirm_label: "Commit".into(),
                danger,
                action: PendingAction::CommitChanges(tab),
            },
        );
    }

    pub(crate) fn commit_changes(&mut self, tab: TabId) {
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        let Some(grid) = table.grid.as_ref() else {
            return;
        };
        let changes = match grid.build_change_set(&table.table) {
            Ok(changes) => changes,
            Err(reason) => {
                self.notify(Level::Error, reason);
                return;
            }
        };
        let source = table.data_source;
        let name = table.table.to_string();
        let operation = Operation::TableChanges {
            table: &name,
            changes: changes.changes.len(),
        };
        if !self.guard_write(source, &operation, true, || {
            PendingAction::CommitChanges(tab)
        }) {
            return;
        }
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        table.loading = true;
        let Some(connection) = self.connection(source) else {
            self.notify(Level::Error, "not connected");
            return;
        };
        self.status = "committing…".into();
        self.spawn(async move {
            let result = match loaders::sql(connection.as_ref()) {
                Ok(sql) => sql.apply_changes(&changes).await,
                Err(error) => Err(error),
            };
            AppEvent::ChangesApplied { tab, result }
        });
    }

    pub(crate) fn on_changes_applied(
        &mut self,
        tab: TabId,
        result: Result<ChangeReport, DriverError>,
    ) {
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        table.loading = false;
        match result {
            Ok(report) => {
                if let Some(grid) = table.grid.as_mut() {
                    grid.discard_changes();
                }
                self.notify(
                    Level::Success,
                    format!(
                        "committed: {} updated, {} inserted, {} deleted",
                        report.updated, report.inserted, report.deleted
                    ),
                );
                self.fetch_table_page(tab);
            }
            Err(error) => {
                let detail = error
                    .detail
                    .clone()
                    .map(|detail| format!(" — {detail}"))
                    .unwrap_or_default();
                self.notify(
                    Level::Error,
                    format!(
                        "commit failed, transaction rolled back: {}{detail}",
                        error.message
                    ),
                );
            }
        }
    }

    /// Keys routed to the table before the keymap: inline cell editing and the filter
    /// completion popup.
    pub(crate) fn intercept_table_key(&mut self, chord: KeyChord) -> bool {
        let Some(tab) = self.workspace.active_tab_id() else {
            return false;
        };
        let Some(table) = self.table_tab(tab) else {
            return false;
        };
        if let Some(grid) = table.grid.as_mut()
            && let Some(editor) = grid.editing.as_mut()
        {
            match chord.code {
                KeyCode::Enter => grid.commit_edit(),
                KeyCode::Tab => {
                    grid.commit_edit();
                    grid.move_columns(1);
                }
                KeyCode::Esc => grid.cancel_edit(),
                _ => {
                    edit_line(&mut editor.input, chord);
                }
            }
            return true;
        }
        if table.focus != TableFocus::Filter {
            return false;
        }
        if let Some(popup) = table.completion.as_mut() {
            let count = popup.items.len().max(1);
            match chord.code {
                KeyCode::Up => {
                    popup.selected = (popup.selected + count - 1) % count;
                    return true;
                }
                KeyCode::Down => {
                    popup.selected = (popup.selected + 1) % count;
                    return true;
                }
                KeyCode::Tab | KeyCode::Enter
                    if !popup.from_history || chord.code == KeyCode::Tab =>
                {
                    let popup = table.completion.take();
                    if let Some(popup) = popup
                        && let Some(item) = popup.items.get(popup.selected)
                    {
                        let insert = item.insert_text.clone();
                        table
                            .query
                            .filter_input
                            .replace_before_cursor(popup.replace_start, &insert);
                    }
                    return true;
                }
                KeyCode::Esc => {
                    table.completion = None;
                    return true;
                }
                _ => {}
            }
        }
        match chord.code {
            KeyCode::Enter => {
                table.completion = None;
                self.apply_table_filter(tab);
            }
            KeyCode::Esc => {
                table.completion = None;
                table.focus = TableFocus::Grid;
            }
            KeyCode::Up | KeyCode::Down => self.open_filter_history(tab),
            KeyCode::Char(' ') if chord.modifiers.ctrl => self.update_filter_completion(tab, true),
            _ => {
                if !edit_line(&mut table.query.filter_input, chord) {
                    return false;
                }
                let typed_word =
                    matches!(chord.code, KeyCode::Char(ch) if ch.is_alphanumeric() || ch == '_');
                if typed_word {
                    self.update_filter_completion(tab, false);
                } else if let Some(table) = self.table_tab(tab) {
                    table.completion = None;
                }
            }
        }
        true
    }

    pub(crate) fn table_raw_key(&mut self, chord: KeyChord) {
        if chord.code != KeyCode::Esc {
            return;
        }
        if let Some(grid) = self.active_tab_mut().and_then(Tab::focused_grid_mut) {
            grid.selected.clear();
        }
    }

    fn apply_table_filter(&mut self, tab: TabId) {
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        if table
            .grid
            .as_ref()
            .is_some_and(GridState::has_pending_changes)
        {
            self.notify(
                Level::Warning,
                "commit or discard pending changes before filtering",
            );
            return;
        }
        let filter = table.query.apply_filter_input();
        table.focus = TableFocus::Grid;
        let key = table.filter_key();
        if let Some(filter) = filter {
            self.filter_history
                .entry(key)
                .or_insert_with(|| RecentList::new(FILTER_HISTORY_SIZE))
                .push(&filter);
            self.persist_filter_history();
        }
        self.fetch_table_page(tab);
    }

    pub(crate) fn persist_filter_history(&mut self) {
        if let Some(project) = &self.project
            && let Err(error) = self
                .context
                .store
                .save_filters(project.id, &self.filter_history)
        {
            tracing::warn!(%error, "failed to save filter history");
        }
    }

    fn open_filter_history(&mut self, tab: TabId) {
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        let key = table.filter_key();
        let items: Vec<PickerItem> = self
            .filter_history
            .get(&key)
            .map(|history| {
                history
                    .items()
                    .map(|filter| PickerItem {
                        label: filter.to_owned(),
                        detail: String::new(),
                        hint: String::new(),
                        value: PickerValue::Filter(filter.to_owned()),
                    })
                    .collect()
            })
            .unwrap_or_default();
        if items.is_empty() {
            self.status = "no filter history for this table yet".into();
            return;
        }
        self.overlays.push(Overlay::Picker(PickerState::new(
            "Filter history",
            PickerPurpose::FilterHistory,
            items,
            "Enter apply · Esc cancel",
        )));
    }

    pub(crate) fn apply_filter_from_history(&mut self, filter: &str) {
        let Some(tab) = self.workspace.active_tab_id() else {
            return;
        };
        if let Some(table) = self.table_tab(tab) {
            table.query.filter_input.set(filter);
        }
        self.apply_table_filter(tab);
    }

    /// Completion for the WHERE filter: the table's columns, operators/keywords and
    /// functions via the SQL completion engine, plus values of the referenced column taken
    /// from the rows already loaded.
    fn update_filter_completion(&mut self, tab: TabId, force: bool) {
        let dialect = match self.workspace.tab(tab) {
            Some(Tab::Table(table)) => self.dialect(table.data_source),
            _ => return,
        };
        let Some(table) = self.table_tab(tab) else {
            return;
        };
        let Some(details) = table.details.as_ref() else {
            return;
        };
        let mut catalog = CompletionCatalog::empty(dialect);
        catalog.tables.push(CatalogTable {
            schema: None,
            name: details.table.name.clone(),
            kind: ObjectKind::Table,
            columns: Some(loaders::catalog_columns(details)),
        });
        let prefix = format!(
            "SELECT * FROM {} WHERE ",
            dialect.quote_identifier(&details.table.name)
        );
        let filter = table.query.filter_input.text().to_owned();
        let cursor = table.query.filter_input.byte_cursor();
        let source = format!("{prefix}{filter}");
        let result = complete(&catalog, &source, prefix.len() + cursor);
        let mut items = value_suggestions(table, &filter[..cursor], &result.prefix, dialect);
        items.extend(result.items);
        let replace_start = filter[..result
            .replace_start
            .saturating_sub(prefix.len())
            .min(filter.len())]
            .chars()
            .count();
        let show = !items.is_empty() && (force || !result.prefix.is_empty());
        table.completion = show.then_some(FilterCompletion {
            items,
            selected: 0,
            replace_start,
            from_history: false,
        });
    }

    pub(crate) fn sort_editor_changed(&mut self, tab: TabId) {
        self.apply_table_sort(tab);
    }
}

fn table_editability(details: Option<&TableDetails>, read_only: bool) -> Editability {
    if read_only {
        return Editability::ReadOnly("the data source's execution policy is read-only".into());
    }
    let Some(details) = details else {
        return Editability::ReadOnly("table metadata unavailable".into());
    };
    if details.kind != ObjectKind::Table {
        return Editability::ReadOnly(format!("{}s are read-only", details.kind.label()));
    }
    match details.row_identity() {
        Some(key_columns) => Editability::Editable { key_columns },
        None => Editability::ReadOnly(
            "no primary key or unique NOT NULL index: rows cannot be identified safely".into(),
        ),
    }
}

/// Suggests literal values when the filter ends in `<column> <operator> <prefix>`, using
/// the distinct values of that column in the currently loaded page.
fn value_suggestions(
    table: &TableTab,
    before_cursor: &str,
    prefix: &str,
    dialect: relix_core::Dialect,
) -> Vec<CompletionItem> {
    let Some(grid) = table.grid.as_ref() else {
        return Vec::new();
    };
    let before_prefix = &before_cursor[..before_cursor.len() - prefix.len()];
    let quote_open = before_prefix.ends_with('\'');
    let head = before_prefix.trim_end().trim_end_matches('\'').trim_end();
    let operators = ["=", "<>", "!=", ">=", "<=", ">", "<", "like", "ilike"];
    let Some(operator) = operators
        .iter()
        .find(|operator| head.to_lowercase().ends_with(*operator))
    else {
        return Vec::new();
    };
    let column_text = head[..head.len() - operator.len()].trim_end();
    let column_name: String = column_text
        .chars()
        .rev()
        .take_while(|ch| ch.is_alphanumeric() || *ch == '_' || *ch == '"' || *ch == '`')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>()
        .trim_matches(['"', '`'])
        .to_owned();
    let Some(column) = grid.column_index(&column_name) else {
        return Vec::new();
    };
    let mut seen: Vec<String> = Vec::new();
    for row in &grid.rows {
        let Some(text) = row.values.get(column).and_then(Value::to_plain_string) else {
            continue;
        };
        if !seen.contains(&text) && text.to_lowercase().starts_with(&prefix.to_lowercase()) {
            seen.push(text);
        }
        if seen.len() == VALUE_SUGGESTIONS {
            break;
        }
    }
    let numeric = row_value_is_numeric(grid, column);
    seen.into_iter()
        .map(|text| {
            let insert = match (numeric, quote_open) {
                (true, _) => text.clone(),
                (false, true) => dialect.quote_literal(&text)[1..].to_owned(),
                (false, false) => dialect.quote_literal(&text),
            };
            CompletionItem::new(text, insert, CompletionKind::Value, None)
        })
        .collect()
}

fn row_value_is_numeric(grid: &GridState, column: usize) -> bool {
    grid.rows
        .iter()
        .find_map(|row| row.values.get(column).filter(|value| !value.is_null()))
        .is_some_and(Value::is_numeric)
}
