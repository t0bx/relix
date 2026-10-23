/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use crate::command::Command;
use crate::editor_tab::{EditorFocus, ResultKind};
use crate::events::AppEvent;
use crate::loaders;
use crate::notifications::Level;
use crate::overlay::PromptAction;
use crate::requests::RequestId;
use crate::tab::Tab;
use crate::workspace::TabId;
use relix_config::{KeyChord, KeyCode};
use relix_core::{RowBatch, StatementOutcome};
use relix_driver_api::{DriverError, ExecuteOptions};
use relix_query::wrapping::{ResultView, wrap_query};

impl App {
    pub(crate) fn results_command(&mut self, command: Command) {
        if self.grid_command(command) {
            return;
        }
        let Some(tab) = self.workspace.active_tab_id() else {
            return;
        };
        match command {
            Command::FocusEditor => self.set_editor_focus(EditorFocus::Text),
            Command::NextPage | Command::PreviousPage => {
                self.page_result(tab, command == Command::NextPage)
            }
            Command::SortColumn | Command::AddSortColumn => {
                self.sort_result(tab, command == Command::AddSortColumn)
            }
            Command::Filter => {
                let current = self
                    .active_result_query()
                    .and_then(|query| query.applied_filter.clone())
                    .unwrap_or_default();
                self.open_prompt(
                    "Filter results (WHERE)",
                    "re-runs the query as a sub-select",
                    PromptAction::ResultFilter { tab },
                    &current,
                );
            }
            Command::Refresh => self.requery_result(tab),
            Command::EditCell | Command::InsertRow | Command::DeleteRows | Command::SetNull => {
                self.notify(
                    Level::Info,
                    "query results are read-only — open the table from the explorer to edit rows",
                );
            }
            _ => {}
        }
    }

    pub(crate) fn results_raw_key(&mut self, chord: KeyChord) {
        let Some(Tab::Editor(editor)) = self.active_tab_mut() else {
            return;
        };
        match chord.code {
            KeyCode::Esc => {
                if let Some(grid) = editor.active_grid_mut()
                    && !grid.selected.is_empty()
                {
                    grid.selected.clear();
                    return;
                }
                editor.focus = EditorFocus::Text;
            }
            KeyCode::Char('}') if !editor.results.is_empty() => {
                editor.active_result = (editor.active_result + 1) % editor.results.len();
            }
            KeyCode::Char('{') if !editor.results.is_empty() => {
                let count = editor.results.len();
                editor.active_result = (editor.active_result + count - 1) % count;
            }
            _ => {}
        }
    }

    fn active_result_query(&self) -> Option<&crate::result_query::ResultQuery> {
        let Some(Tab::Editor(editor)) = self.active_tab() else {
            return None;
        };
        match &editor.active_result()?.kind {
            ResultKind::Rows { query, .. } => Some(query),
            _ => None,
        }
    }

    fn page_result(&mut self, tab: TabId, forward: bool) {
        let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab) else {
            return;
        };
        let Some(entry) = editor.active_result_mut() else {
            return;
        };
        let ResultKind::Rows {
            query, wrappable, ..
        } = &mut entry.kind
        else {
            return;
        };
        if !*wrappable {
            self.notify(
                Level::Info,
                "paging needs a plain SELECT; raise query.row_limit to see more rows",
            );
            return;
        }
        let moved = if forward {
            query.next_page()
        } else {
            query.previous_page()
        };
        if moved {
            self.requery_result(tab);
        }
    }

    fn sort_result(&mut self, tab: TabId, multi: bool) {
        let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab) else {
            return;
        };
        let Some(entry) = editor.active_result_mut() else {
            return;
        };
        let ResultKind::Rows {
            grid,
            query,
            wrappable,
            ..
        } = &mut entry.kind
        else {
            return;
        };
        let Some(column) = grid.current_column().map(|column| column.meta.name.clone()) else {
            return;
        };
        if multi {
            query.add_sort(&column);
        } else {
            query.cycle_sort(&column);
        }
        if *wrappable {
            self.requery_result(tab);
        } else {
            let keys = query.sort.clone();
            grid.sort_locally(&keys);
            self.status = "sorted loaded rows locally (statement cannot be re-queried)".into();
        }
    }

    pub(crate) fn apply_result_filter(&mut self, tab: TabId, filter: &str) {
        let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab) else {
            return;
        };
        let Some(entry) = editor.active_result_mut() else {
            return;
        };
        let ResultKind::Rows {
            query, wrappable, ..
        } = &mut entry.kind
        else {
            return;
        };
        if !*wrappable {
            self.notify(Level::Info, "filtering needs a plain SELECT statement");
            return;
        }
        query.filter_input.set(filter);
        query.apply_filter_input();
        self.requery_result(tab);
    }

    /// Re-runs the active result's statement wrapped in a sub-select with the current
    /// filter, sort and page applied on the server.
    fn requery_result(&mut self, tab: TabId) {
        let request = self.next_request();
        let Some(Tab::Editor(editor)) = self.workspace.tab(tab) else {
            return;
        };
        let Some(source) = editor.data_source else {
            return;
        };
        let dialect = self.dialect(source);
        let Some(connection) = self.connection(source) else {
            self.notify(Level::Warning, "not connected");
            return;
        };
        let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab) else {
            return;
        };
        let index = editor.active_result;
        let Some(entry) = editor.results.get_mut(index) else {
            return;
        };
        let ResultKind::Rows { query, loading, .. } = &mut entry.kind else {
            return;
        };
        let view = ResultView {
            filter: query.applied_filter.as_deref(),
            sort: &query.sort,
            offset: query.offset,
            limit: query.page_size,
        };
        let Some(sql) = wrap_query(&entry.statement, dialect, &view) else {
            return;
        };
        let page_size = query.page_size as usize;
        *loading = true;
        entry.request = Some(request);
        self.spawn(async move {
            let result = match loaders::sql(connection.as_ref()) {
                Ok(executor) => executor
                    .execute(
                        &sql,
                        &ExecuteOptions {
                            row_limit: page_size + 1,
                        },
                    )
                    .await
                    .and_then(|outcome| match outcome {
                        StatementOutcome::Rows(batch) => Ok(trim_lookahead(batch, page_size)),
                        _ => Err(DriverError::internal("re-query did not return rows")),
                    }),
                Err(error) => Err(error),
            };
            AppEvent::ResultPage {
                tab,
                index,
                request,
                result,
            }
        });
    }

    pub(crate) fn on_result_page(
        &mut self,
        tab: TabId,
        index: usize,
        request: RequestId,
        result: Result<RowBatch, DriverError>,
    ) {
        let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab) else {
            return;
        };
        let Some(entry) = editor
            .results
            .get_mut(index)
            .filter(|entry| entry.request == Some(request))
        else {
            return;
        };
        entry.request = None;
        let ResultKind::Rows {
            grid,
            query,
            loading,
            ..
        } = &mut entry.kind
        else {
            return;
        };
        *loading = false;
        match result {
            Ok(batch) => {
                query.has_more = batch.has_more;
                grid.replace_rows(&batch);
                self.status = format!("rows {}", query.page_label(batch.rows.len()));
            }
            Err(error) => self.notify(Level::Error, error.to_string()),
        }
    }
}

/// The wrapped query asks for one extra row; it only signals that another page exists.
fn trim_lookahead(mut batch: RowBatch, page_size: usize) -> RowBatch {
    if batch.rows.len() > page_size {
        batch.rows.truncate(page_size);
        batch.has_more = true;
    }
    batch
}
