/*
 * @author t0bx
 * @since 23.10.2026
 */

mod command_dispatch;
mod connections;
mod datasources;
mod drivers;
mod editor_actions;
mod event_dispatch;
mod explorer_actions;
mod focus;
mod grid_actions;
mod keyvalue_actions;
mod overlay_input;
mod pickers;
mod projects;
mod results_actions;
mod session;
mod table_actions;
#[cfg(test)]
mod tests;

use crate::command::KeyContext;
use crate::editor_tab::EditorFocus;
use crate::events::{AppEvent, InputEvent};
use crate::explorer::ExplorerState;
use crate::keymap::{KeyResolution, Keymap};
use crate::keyvalue_tab::KeyValueFocus;
use crate::notifications::{Level, Notifications};
use crate::overlay::Overlay;
use crate::requests::{RequestCounter, RequestId};
use crate::services::{Clipboard, DriverService};
use crate::tab::Tab;
use crate::table_tab::TableFocus;
use crate::workspace::{Area, PaneId, TabId, Workspace};
pub use connections::{ConnectionState, ConnectionStatus};
use relix_config::{AppPaths, GlobalSettings, KeyChord, Theme, ThemeCatalog};
use relix_core::{DataSourceId, Secret};
use relix_editor::CompletionCatalog;
use relix_project::{Project, ProjectStore, RecentProjects, SecretStore};
use relix_query::{QueryHistory, RecentList};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub const EVENT_QUEUE_CAPACITY: usize = 1024;
const SESSION_AUTOSAVE: Duration = Duration::from_secs(30);

pub struct AppContext {
    pub paths: AppPaths,
    pub store: ProjectStore,
    pub secrets: Arc<dyn SecretStore>,
    pub drivers: Arc<dyn DriverService>,
    pub clipboard: Box<dyn Clipboard>,
    pub runtime: tokio::runtime::Handle,
    pub events: mpsc::Sender<AppEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Workspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    ExplorerRow(usize),
    TabHeader(TabId),
    Pane(PaneId),
    GridRow { pane: PaneId, row: usize },
    KeyRow { pane: PaneId, row: usize },
}

#[derive(Debug, Clone, Default)]
pub struct HitMap {
    regions: Vec<(Area, HitTarget)>,
}

impl HitMap {
    pub fn clear(&mut self) {
        self.regions.clear();
    }

    pub fn push(&mut self, area: Area, target: HitTarget) {
        self.regions.push((area, target));
    }

    /// Returns the most specific (last registered) region containing the point.
    pub fn hit(&self, column: u16, row: u16) -> Option<HitTarget> {
        self.regions
            .iter()
            .rev()
            .find(|(area, _)| {
                column >= area.x
                    && column < area.x.saturating_add(area.width)
                    && row >= area.y
                    && row < area.y.saturating_add(area.height)
            })
            .map(|(_, target)| *target)
    }
}

pub struct App {
    pub settings: GlobalSettings,
    pub themes: ThemeCatalog,
    pub theme: Theme,
    pub keymap: Keymap,
    pub project: Option<Project>,
    pub recent: RecentProjects,
    pub workspace: Workspace<Tab>,
    pub explorer: ExplorerState,
    pub sidebar_visible: bool,
    pub sidebar_width: u16,
    pub focus: Focus,
    pub overlays: Vec<Overlay>,
    pub notifications: Notifications,
    pub connections: HashMap<DataSourceId, ConnectionState>,
    pub catalogs: HashMap<DataSourceId, CompletionCatalog>,
    pub history: QueryHistory,
    pub filter_history: BTreeMap<String, RecentList>,
    pub pending_keys: Vec<KeyChord>,
    pub should_quit: bool,
    pub status: String,
    pub hit_map: HitMap,
    session_passwords: HashMap<DataSourceId, Secret>,
    requests: RequestCounter,
    context: AppContext,
    last_session_save: Instant,
    pub(crate) catalog_requests: Vec<(DataSourceId, Option<String>, String)>,
}

impl App {
    pub fn new(
        context: AppContext,
        settings: GlobalSettings,
        themes: ThemeCatalog,
        warnings: Vec<String>,
    ) -> Self {
        let (keymap, keymap_warnings) = Keymap::with_overrides(&settings.keybindings);
        let mut theme = themes.resolve(&settings.theme);
        let unknown_colors = theme.apply_overrides(&settings.theme_overrides);
        let recent = RecentProjects::load(&context.paths.recent_projects_file());
        let mut app = Self {
            sidebar_width: settings.ui.sidebar_width,
            history: QueryHistory::new(settings.query.history_limit),
            settings,
            themes,
            theme,
            keymap,
            project: None,
            recent,
            workspace: Workspace::new(),
            explorer: ExplorerState::default(),
            sidebar_visible: true,
            focus: Focus::Sidebar,
            overlays: Vec::new(),
            notifications: Notifications::default(),
            connections: HashMap::new(),
            catalogs: HashMap::new(),
            filter_history: BTreeMap::new(),
            pending_keys: Vec::new(),
            should_quit: false,
            status: String::new(),
            hit_map: HitMap::default(),
            session_passwords: HashMap::new(),
            requests: RequestCounter::default(),
            context,
            last_session_save: Instant::now(),
            catalog_requests: Vec::new(),
        };
        for warning in warnings.into_iter().chain(keymap_warnings) {
            app.notify(Level::Warning, warning);
        }
        for color in unknown_colors {
            app.notify(
                Level::Warning,
                format!("unknown theme color '{color}' in [theme_overrides]"),
            );
        }
        app
    }

    pub fn context(&self) -> &AppContext {
        &self.context
    }

    pub fn notify(&mut self, level: Level, message: impl Into<String>) {
        self.notifications.push(level, message);
    }

    pub(crate) fn next_request(&mut self) -> RequestId {
        self.requests.allocate()
    }

    /// Runs a future on the async runtime and delivers its event to the UI loop. The
    /// event queue is bounded, so a flood of results applies back-pressure to tasks
    /// instead of growing memory without limit.
    pub(crate) fn spawn<F>(&self, task: F)
    where
        F: Future<Output = AppEvent> + Send + 'static,
    {
        let sender = self.context.events.clone();
        self.context.runtime.spawn(async move {
            let event = task.await;
            let _ = sender.send(event).await;
        });
    }

    pub(crate) fn event_sender(&self) -> mpsc::Sender<AppEvent> {
        self.context.events.clone()
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.workspace.active_tab()
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.workspace.active_tab_mut()
    }

    pub fn handle_input(&mut self, input: InputEvent) {
        match input {
            InputEvent::Key(chord) => self.handle_key(chord),
            InputEvent::Paste(text) => self.paste(&text),
            InputEvent::Mouse(mouse) => self.handle_mouse(mouse),
            InputEvent::Tick => self.tick(),
        }
    }

    pub fn tick(&mut self) {
        self.notifications.expire();
        if self.last_session_save.elapsed() >= SESSION_AUTOSAVE {
            self.last_session_save = Instant::now();
            if let Err(error) = self.save_session() {
                self.notify(Level::Error, format!("session autosave failed: {error}"));
            }
        }
    }

    /// Whether printable keys should be inserted as text rather than looked up as
    /// shortcuts, which is how conflicts like `j` (navigate vs. type) are resolved.
    pub fn is_text_input_focused(&self) -> bool {
        match self.focus {
            Focus::Sidebar => self.explorer.filtering,
            Focus::Workspace => match self.active_tab() {
                Some(Tab::Editor(editor)) => editor.focus == EditorFocus::Text,
                Some(Tab::Table(table)) => {
                    table.focus == TableFocus::Filter
                        || table
                            .grid
                            .as_ref()
                            .is_some_and(|grid| grid.editing.is_some())
                }
                Some(Tab::KeyValue(tab)) => {
                    matches!(tab.focus, KeyValueFocus::Pattern | KeyValueFocus::Console)
                }
                None => false,
            },
        }
    }

    pub fn key_contexts(&self) -> Vec<KeyContext> {
        let navigable = |primary: KeyContext| {
            vec![
                primary,
                KeyContext::Navigable,
                KeyContext::NonText,
                KeyContext::Global,
            ]
        };
        if self.is_text_input_focused() {
            return match self.active_tab() {
                Some(Tab::Editor(editor))
                    if self.focus == Focus::Workspace && editor.focus == EditorFocus::Text =>
                {
                    vec![KeyContext::Editor, KeyContext::Global]
                }
                _ => vec![KeyContext::Global],
            };
        }
        match (self.focus, self.active_tab()) {
            (Focus::Sidebar, _) => navigable(KeyContext::Explorer),
            (Focus::Workspace, Some(Tab::Editor(_) | Tab::Table(_))) => navigable(KeyContext::Grid),
            (Focus::Workspace, Some(Tab::KeyValue(_))) => navigable(KeyContext::KeyValue),
            (Focus::Workspace, None) => vec![KeyContext::NonText, KeyContext::Global],
        }
    }

    pub fn handle_key(&mut self, chord: KeyChord) {
        if !self.overlays.is_empty() {
            self.handle_overlay_key(chord);
            return;
        }
        if self.focus == Focus::Workspace
            && (self.intercept_editor_key(chord) || self.intercept_table_key(chord))
        {
            return;
        }
        let text_mode = self.is_text_input_focused();
        if text_mode && chord.is_text_input() && self.pending_keys.is_empty() {
            self.handle_raw_key(chord);
            return;
        }
        let contexts = self.key_contexts();
        match self.keymap.resolve(&contexts, &self.pending_keys, chord) {
            KeyResolution::Command(command) => {
                self.pending_keys.clear();
                self.execute_command(command);
            }
            KeyResolution::Pending => self.pending_keys.push(chord),
            KeyResolution::NoMatch if !self.pending_keys.is_empty() => self.pending_keys.clear(),
            KeyResolution::NoMatch => self.handle_raw_key(chord),
        }
    }

    pub fn handle_event(&mut self, event: AppEvent) {
        self.dispatch_event(event);
    }

    pub fn shutdown(&mut self) {
        if let Err(error) = self.save_session() {
            tracing::error!(%error, "failed to save session on shutdown");
        }
        self.disconnect_all();
    }
}
