/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use super::connections::ConnectionStatus;
use crate::editor_tab::EditorTab;
use crate::grid::{GridState, GridView};
use crate::input::LineInput;
use crate::keyvalue_tab::KeyValueTab;
use crate::notifications::Level;
use crate::tab::Tab;
use crate::table_tab::TableTab;
use crate::workspace::{LayoutNode, Pane, PaneId, TabId, Workspace};
use relix_core::ProjectId;
use relix_editor::Position;
use relix_project::{
    EditorSnapshot, KeyValueSnapshot, LayoutSnapshot, PaneSnapshot, ProjectError, SessionState,
    TabSnapshot, TableSnapshot,
};
use std::collections::BTreeMap;

impl App {
    /// Persists the workspace: layout, tabs (unsaved editor content goes to draft files),
    /// connected data sources and the expanded explorer tree.
    pub fn save_session(&mut self) -> Result<(), ProjectError> {
        let Some(project) = self.project.as_ref().map(|project| project.id) else {
            return Ok(());
        };
        let mut drafts = Vec::new();
        let layout = self.snapshot_layout(project, self.workspace.layout(), &mut drafts)?;
        let focused_pane = self
            .workspace
            .pane_order()
            .iter()
            .position(|pane| *pane == self.workspace.focused_pane())
            .unwrap_or(0);
        let connected = self
            .connections
            .iter()
            .filter(|(_, state)| state.status == ConnectionStatus::Connected)
            .map(|(id, _)| *id)
            .collect();
        let session = SessionState {
            layout,
            focused_pane,
            sidebar_visible: self.sidebar_visible,
            sidebar_width: Some(self.sidebar_width),
            connected,
            expanded_nodes: self.explorer.expanded_keys(),
            ..SessionState::default()
        };
        self.context.store.save_session(project, &session)?;
        self.context.store.prune_drafts(project, &drafts)
    }

    fn snapshot_layout(
        &self,
        project: ProjectId,
        node: &LayoutNode,
        drafts: &mut Vec<String>,
    ) -> Result<LayoutSnapshot, ProjectError> {
        Ok(match node {
            LayoutNode::Leaf(pane) => {
                let Some(pane) = self.workspace.pane(*pane) else {
                    return Ok(LayoutSnapshot::default());
                };
                let mut tabs = Vec::new();
                for tab in &pane.tabs {
                    if let Some(tab) = self.workspace.tab(*tab) {
                        tabs.push(self.snapshot_tab(project, tab, drafts)?);
                    }
                }
                LayoutSnapshot::Pane(PaneSnapshot {
                    tabs,
                    active: pane.active,
                })
            }
            LayoutNode::Split {
                direction,
                ratio,
                first,
                second,
            } => LayoutSnapshot::Split {
                direction: *direction,
                ratio: *ratio,
                first: Box::new(self.snapshot_layout(project, first, drafts)?),
                second: Box::new(self.snapshot_layout(project, second, drafts)?),
            },
        })
    }

    fn snapshot_tab(
        &self,
        project: ProjectId,
        tab: &Tab,
        drafts: &mut Vec<String>,
    ) -> Result<TabSnapshot, ProjectError> {
        Ok(match tab {
            Tab::Editor(editor) => {
                let draft = if editor.is_dirty() {
                    self.context.store.write_draft(
                        project,
                        &editor.draft_id,
                        &editor.buffer.text(),
                    )?;
                    drafts.push(editor.draft_id.clone());
                    Some(editor.draft_id.clone())
                } else {
                    None
                };
                let cursor = editor.buffer.cursor();
                TabSnapshot::Editor(EditorSnapshot {
                    title: editor.title.clone(),
                    script: editor.path.clone(),
                    draft,
                    data_source: editor.data_source,
                    cursor_line: cursor.line,
                    cursor_column: cursor.column,
                })
            }
            Tab::Table(table) => TabSnapshot::Table(TableSnapshot {
                data_source: table.data_source,
                table: table.table.clone(),
                filter: table.query.applied_filter.clone(),
                sort: table.query.sort.clone(),
                json_view: table
                    .grid
                    .as_ref()
                    .map_or(table.restore_view, |grid| grid.view)
                    == GridView::Json,
                hidden_columns: table.grid.as_ref().map_or_else(
                    || table.restore_hidden.clone(),
                    GridState::hidden_column_names,
                ),
            }),
            Tab::KeyValue(tab) => TabSnapshot::KeyValue(KeyValueSnapshot {
                data_source: tab.data_source,
                pattern: tab.pattern.text().to_owned(),
            }),
        })
    }

    pub(crate) fn restore_session(&mut self) {
        let Some(project) = self.project.clone() else {
            return;
        };
        let session = match self.context.store.load_session(project.id) {
            Ok(session) => session.unwrap_or_default(),
            Err(error) => {
                self.notify(
                    Level::Warning,
                    format!("session could not be restored: {error}"),
                );
                SessionState::default()
            }
        };
        let mut builder = RestoreBuilder::default();
        let layout = builder.layout(self, project.id, &session.layout);
        let focused = layout
            .leaves()
            .get(session.focused_pane)
            .copied()
            .unwrap_or(1);
        self.workspace = Workspace::restore(layout, builder.panes, builder.tabs, focused);
        self.sidebar_visible = session.sidebar_visible;
        if let Some(width) = session.sidebar_width {
            self.sidebar_width = width.clamp(16, 120);
        }
        self.explorer.restore_expanded(session.expanded_nodes);
        self.focus = if self.workspace.active_tab().is_some() {
            super::Focus::Workspace
        } else {
            super::Focus::Sidebar
        };
        let tabs: Vec<TabId> = self.workspace.tabs().map(|(id, _)| id).collect();
        for tab in tabs {
            self.load_tab_content(tab);
        }
        for source in session.connected {
            if project.data_source(source).is_some() && self.connection(source).is_none() {
                self.connect(source);
            }
        }
    }

    fn restore_editor(&self, project: ProjectId, snapshot: &EditorSnapshot) -> EditorTab {
        let store = &self.context.store;
        let draft = snapshot
            .draft
            .as_deref()
            .and_then(|draft| store.read_draft(project, draft).ok().flatten());
        let script_path = snapshot
            .script
            .as_ref()
            .map(|script| store.resolve_script(project, script));
        let saved = script_path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok());
        let text = draft.clone().or_else(|| saved.clone()).unwrap_or_default();
        let source = snapshot
            .data_source
            .filter(|source| self.data_source(*source).is_some());
        let mut editor = EditorTab::new(snapshot.title.clone(), source, &text);
        editor.path = script_path;
        if let Some(draft_id) = &snapshot.draft {
            editor.draft_id = draft_id.clone();
        }
        if draft.is_some() && draft != saved {
            editor.buffer.mark_unsaved();
        }
        editor.buffer.set_cursor(
            Position::new(snapshot.cursor_line, snapshot.cursor_column),
            false,
        );
        editor
    }

    fn restore_table(&self, snapshot: &TableSnapshot) -> Option<TableTab> {
        self.data_source(snapshot.data_source)?;
        let mut table = TableTab::new(
            snapshot.data_source,
            snapshot.table.clone(),
            self.settings.query.page_size,
        );
        table.query.sort = snapshot.sort.clone();
        if let Some(filter) = &snapshot.filter {
            table.query.filter_input = LineInput::with_text(filter);
            table.query.applied_filter = Some(filter.clone());
        }
        table.restore_hidden = snapshot.hidden_columns.clone();
        table.restore_view = if snapshot.json_view {
            GridView::Json
        } else {
            GridView::Table
        };
        Some(table)
    }
}

#[derive(Default)]
struct RestoreBuilder {
    panes: Vec<Pane>,
    tabs: BTreeMap<TabId, Tab>,
    next_id: u32,
}

impl RestoreBuilder {
    fn allocate(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    fn layout(&mut self, app: &App, project: ProjectId, snapshot: &LayoutSnapshot) -> LayoutNode {
        match snapshot {
            LayoutSnapshot::Pane(pane) => LayoutNode::Leaf(self.pane(app, project, pane)),
            LayoutSnapshot::Split {
                direction,
                ratio,
                first,
                second,
            } => LayoutNode::Split {
                direction: *direction,
                ratio: (*ratio).clamp(15, 85),
                first: Box::new(self.layout(app, project, first)),
                second: Box::new(self.layout(app, project, second)),
            },
        }
    }

    fn pane(&mut self, app: &App, project: ProjectId, snapshot: &PaneSnapshot) -> PaneId {
        let id = self.allocate();
        let mut tabs = Vec::new();
        for tab in &snapshot.tabs {
            let restored = match tab {
                TabSnapshot::Editor(editor) => {
                    Some(Tab::Editor(Box::new(app.restore_editor(project, editor))))
                }
                TabSnapshot::Table(table) => app
                    .restore_table(table)
                    .map(|table| Tab::Table(Box::new(table))),
                TabSnapshot::KeyValue(kv) => app.data_source(kv.data_source).map(|_| {
                    let mut tab = KeyValueTab::new(kv.data_source);
                    tab.pattern = LineInput::with_text(&kv.pattern);
                    Tab::KeyValue(Box::new(tab))
                }),
            };
            if let Some(restored) = restored {
                let tab_id = self.allocate();
                self.tabs.insert(tab_id, restored);
                tabs.push(tab_id);
            }
        }
        let active = snapshot.active.min(tabs.len().saturating_sub(1));
        self.panes.push(Pane { id, tabs, active });
        id
    }
}
