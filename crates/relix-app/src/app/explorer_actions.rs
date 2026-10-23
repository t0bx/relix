/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use super::connections::{ConnectionStatus, Deferred};
use crate::command::Command;
use crate::events::AppEvent;
use crate::explorer::{NodeId, NodeKind};
use crate::loaders;
use crate::notifications::Level;
use crate::overlay::{Overlay, ViewerState};
use relix_config::{KeyChord, KeyCode};
use relix_core::DataSourceId;
use relix_driver_api::DriverError;

const EXPLORER_PAGE: isize = 15;

impl App {
    pub(crate) fn refresh_explorer_roots(&mut self) {
        let roots: Vec<(DataSourceId, String, String)> = self
            .project
            .as_ref()
            .map(|project| {
                project
                    .data_sources
                    .iter()
                    .map(|source| {
                        (
                            source.id,
                            source.name.clone(),
                            self.driver_name(&source.driver),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.explorer.set_roots(&roots);
    }

    pub(crate) fn selected_explorer_source(&self) -> Option<DataSourceId> {
        self.explorer
            .selected_node()
            .and_then(|id| self.explorer.node(id))
            .and_then(|node| node.kind.source())
            .or_else(|| {
                let root = self.explorer.selected_node()?;
                let top = self
                    .explorer
                    .ancestors(root)
                    .last()
                    .copied()
                    .unwrap_or(root);
                self.explorer.node(top)?.kind.source()
            })
    }

    pub(crate) fn explorer_command(&mut self, command: Command) {
        match command {
            Command::MoveUp => self.explorer.move_selection(-1),
            Command::MoveDown => self.explorer.move_selection(1),
            Command::PageUp => self.explorer.move_selection(-EXPLORER_PAGE),
            Command::PageDown => self.explorer.move_selection(EXPLORER_PAGE),
            Command::MoveTop => self.explorer.selected = 0,
            Command::MoveBottom => self.explorer.move_selection(isize::MAX / 2),
            Command::MoveRight => self.expand_selected(),
            Command::Activate => self.activate_selected(false),
            Command::ExplorerOpenInNewPane => self.activate_selected(true),
            Command::MoveLeft | Command::Collapse => self.collapse_selected(),
            Command::Refresh => self.refresh_selected(),
            Command::Filter => self.explorer.filtering = true,
            Command::ExplorerNewQuery => {
                let source = self.selected_explorer_source();
                self.new_editor_tab(source, "");
            }
            Command::EditDataSource => {
                if let Some(source) = self.selected_explorer_source() {
                    self.open_data_source_form(Some(source));
                }
            }
            Command::DuplicateDataSource => self.duplicate_selected_data_source(),
            Command::DeleteDataSource => self.confirm_delete_selected_data_source(),
            Command::InspectObject => self.inspect_selected(),
            _ => {}
        }
    }

    pub(crate) fn explorer_raw_key(&mut self, chord: KeyChord) {
        if !self.explorer.filtering {
            if chord.code == KeyCode::Esc && !self.explorer.filter.is_empty() {
                self.explorer.filter.clear();
            }
            return;
        }
        match chord.code {
            KeyCode::Esc => {
                self.explorer.filter.clear();
                self.explorer.filtering = false;
            }
            KeyCode::Enter => self.explorer.filtering = false,
            KeyCode::Up => self.explorer.move_selection(-1),
            KeyCode::Down => self.explorer.move_selection(1),
            _ => {
                edit_line(&mut self.explorer.filter, chord);
                self.explorer.selected = 0;
            }
        }
    }

    fn expand_selected(&mut self) {
        if let Some(id) = self.explorer.selected_node() {
            if self.explorer.is_expanded(id) {
                self.explorer.move_selection(1);
            } else {
                self.expand_node(id);
            }
        }
    }

    fn collapse_selected(&mut self) {
        let Some(id) = self.explorer.selected_node() else {
            return;
        };
        if self.explorer.is_expanded(id) {
            self.explorer.collapse(id);
        } else {
            self.explorer.select_parent();
        }
    }

    pub(crate) fn expand_node(&mut self, id: NodeId) {
        let Some(key) = self.explorer.node(id).map(|node| node.key.clone()) else {
            return;
        };
        if self.explorer.expand(id) {
            self.load_explorer_key(&key);
        }
    }

    fn refresh_selected(&mut self) {
        let Some(id) = self.explorer.selected_node() else {
            return;
        };
        let Some(node) = self.explorer.node(id) else {
            return;
        };
        let target = if node.kind.is_leaf() {
            node.parent.unwrap_or(id)
        } else {
            id
        };
        if let Some(source) = self
            .explorer
            .node(target)
            .and_then(|node| node.kind.source())
            && matches!(
                self.explorer.node(target).map(|node| &node.kind),
                Some(NodeKind::DataSource(_))
            )
        {
            self.catalogs.remove(&source);
            if self.connection(source).is_some() {
                self.load_catalog(source);
            }
        }
        self.explorer.reset(target);
        self.explorer.select_node(target);
        self.expand_node(target);
    }

    fn activate_selected(&mut self, new_pane: bool) {
        let Some(id) = self.explorer.selected_node() else {
            return;
        };
        let Some(kind) = self.explorer.node(id).map(|node| node.kind.clone()) else {
            return;
        };
        match kind {
            NodeKind::Object { source, info } if info.kind.is_relation() => {
                self.open_table(source, info.object, new_pane);
            }
            NodeKind::Object { .. }
            | NodeKind::Column(_)
            | NodeKind::Index(_)
            | NodeKind::ForeignKey(_) => {
                self.inspect_selected();
            }
            NodeKind::KeySpace(source) => self.open_key_value_tab(source, new_pane),
            NodeKind::DataSource(source)
                if self
                    .capabilities(source)
                    .is_some_and(|capabilities| !capabilities.is_relational()) =>
            {
                self.open_key_value_tab(source, new_pane);
            }
            _ => {
                if self.explorer.is_expanded(id) {
                    self.explorer.collapse(id);
                } else {
                    self.expand_node(id);
                }
            }
        }
    }

    /// Starts loading the children of the node with `key`, connecting first if needed.
    pub(crate) fn load_explorer_key(&mut self, key: &str) {
        let Some(id) = self.explorer.find_key(key) else {
            return;
        };
        let Some(node) = self.explorer.node(id) else {
            return;
        };
        let kind = node.kind.clone();
        let Some(source) = kind.source() else {
            return;
        };
        let Some(capabilities) = self.capabilities(source) else {
            self.explorer.set_failed(id, "driver unavailable".into());
            if self.data_source(source).is_some() {
                self.connect(source);
            }
            return;
        };
        let Some(connection) =
            self.connection_or_defer(source, Deferred::ExpandNode(key.to_owned()))
        else {
            self.explorer.mark_loading(id);
            return;
        };
        self.explorer.mark_loading(id);
        let key = key.to_owned();
        self.spawn(async move {
            let result =
                loaders::load_children(connection.as_ref(), capabilities, &kind, &key).await;
            AppEvent::ExplorerLoaded { key, result }
        });
    }

    pub(crate) fn on_explorer_loaded(
        &mut self,
        key: &str,
        result: Result<Vec<crate::explorer::NewNode>, DriverError>,
    ) {
        let Some(id) = self.explorer.find_key(key) else {
            return;
        };
        match result {
            Ok(children) => {
                self.explorer.set_children(id, children);
                self.resume_explorer_expansions();
            }
            Err(error) => {
                self.explorer.set_failed(id, error.to_string());
                self.notify(Level::Error, error.to_string());
            }
        }
    }

    /// Loads expanded nodes (e.g. restored from the last session) whose data source is
    /// connected but whose children are not loaded yet.
    pub(crate) fn resume_explorer_expansions(&mut self) {
        for id in self.explorer.pending_expansions() {
            let Some(node) = self.explorer.node(id) else {
                continue;
            };
            let connected = node.kind.source().is_some_and(|source| {
                self.connection_status(source) == ConnectionStatus::Connected
            });
            if connected {
                let key = node.key.clone();
                self.load_explorer_key(&key);
            }
        }
    }

    pub(crate) fn explorer_connection_failed(&mut self, source: DataSourceId, error: &DriverError) {
        if let Some(root) = self.explorer.root_for(source) {
            self.explorer.set_failed(root, error.to_string());
        }
    }

    fn inspect_selected(&mut self) {
        let Some(id) = self.explorer.selected_node() else {
            return;
        };
        let Some(kind) = self.explorer.node(id).map(|node| node.kind.clone()) else {
            return;
        };
        match kind {
            NodeKind::Object { source, info } => {
                let Some(connection) = self.connection(source) else {
                    self.notify(Level::Info, "connect to the data source first");
                    return;
                };
                let title = format!("{} {}", info.kind.label(), info.object);
                self.spawn(async move {
                    let result = match loaders::sql(connection.as_ref()) {
                        Ok(sql) => sql.object_definition(&info).await,
                        Err(error) => Err(error),
                    };
                    AppEvent::Definition { title, result }
                });
            }
            NodeKind::Column(column) => {
                let text = format!(
                    "column      {}\ntype        {}\nnullable    {}\ndefault     {}\nprimary key {}\nauto incr.  {}\ncomment     {}",
                    column.name,
                    column.data_type,
                    column.nullable,
                    column.default_value.as_deref().unwrap_or("—"),
                    column.is_primary_key,
                    column.is_auto_increment,
                    column.comment.as_deref().unwrap_or("—"),
                );
                self.overlays
                    .push(Overlay::Viewer(ViewerState::new("Column", &text)));
            }
            NodeKind::Index(index) => {
                let text = format!(
                    "index    {}\ncolumns  {}\nunique   {}\nprimary  {}",
                    index.name,
                    index.columns.join(", "),
                    index.is_unique,
                    index.is_primary
                );
                self.overlays
                    .push(Overlay::Viewer(ViewerState::new("Index", &text)));
            }
            NodeKind::ForeignKey(key) => {
                let text = format!(
                    "foreign key  {}\ncolumns      {}\nreferences   {} ({})",
                    key.name,
                    key.columns.join(", "),
                    key.referenced_table,
                    key.referenced_columns.join(", ")
                );
                self.overlays
                    .push(Overlay::Viewer(ViewerState::new("Foreign key", &text)));
            }
            NodeKind::DataSource(source) => self.show_data_source_info(source),
            _ => {}
        }
    }

    fn show_data_source_info(&mut self, source: DataSourceId) {
        let Some(definition) = self.data_source(source).cloned() else {
            return;
        };
        let server = self
            .connections
            .get(&source)
            .and_then(|state| state.server.clone())
            .map_or_else(
                || "not connected".to_owned(),
                |server| format!("{} {}", server.product, server.version),
            );
        let settings = &definition.settings;
        let location = match &settings.file_path {
            Some(path) => path.display().to_string(),
            None => format!(
                "{}:{}",
                settings.host.as_deref().unwrap_or("localhost"),
                settings
                    .port
                    .map(|port| port.to_string())
                    .unwrap_or_default()
            ),
        };
        let text = format!(
            "name         {}\nengine       {}\nlocation     {location}\ndatabase     {}\nuser         {}\ntls          {}\nenvironment  {}\nserver       {server}\nstatus       {:?}",
            definition.name,
            self.driver_name(&definition.driver),
            settings.database.as_deref().unwrap_or("—"),
            settings.username.as_deref().unwrap_or("—"),
            settings.tls.mode.label(),
            definition.environment.label(),
            self.connection_status(source),
        );
        self.overlays
            .push(Overlay::Viewer(ViewerState::new("Data source", &text)));
    }
}

/// Applies a key press to a single-line input; returns `false` for keys it ignores.
pub(crate) fn edit_line(input: &mut crate::input::LineInput, chord: KeyChord) -> bool {
    let ctrl = chord.modifiers.ctrl;
    let alt = chord.modifiers.alt;
    match chord.code {
        KeyCode::Char('w') if ctrl => input.delete_word_back(),
        KeyCode::Char('u') if ctrl => input.clear(),
        KeyCode::Char('a') if ctrl => input.move_home(),
        KeyCode::Char('e') if ctrl => input.move_end(),
        KeyCode::Char(ch) if !ctrl && !alt => input.insert_char(ch),
        KeyCode::Backspace if ctrl || alt => input.delete_word_back(),
        KeyCode::Backspace => input.backspace(),
        KeyCode::Delete => input.delete(),
        KeyCode::Left if ctrl => input.move_word_left(),
        KeyCode::Right if ctrl => input.move_word_right(),
        KeyCode::Left => input.move_left(),
        KeyCode::Right => input.move_right(),
        KeyCode::Home => input.move_home(),
        KeyCode::End => input.move_end(),
        _ => return false,
    }
    true
}
