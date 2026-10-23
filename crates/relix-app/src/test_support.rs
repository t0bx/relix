/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::app::{App, AppContext, EVENT_QUEUE_CAPACITY};
use crate::editor_tab::EditorTab;
use crate::events::{AppEvent, InputEvent};
use crate::services::{DriverService, MemoryClipboard, StaticDriverService};
use crate::tab::Tab;
use relix_config::{AppPaths, GlobalSettings, KeyChord, KeyCode, ThemeCatalog};
use relix_connections::{ConnectionManager, HostKeyStore, TunnelManager, VpnManager};
use relix_core::{DataSourceDefinition, DataSourceId, DriverId, Environment};
use relix_driver_api::DatabaseDriver;
use relix_driver_sqlite::SqliteDriver;
use relix_project::{MemorySecretStore, ProjectStore};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

const SETTLE_TIMEOUT: Duration = Duration::from_millis(400);

/// Drives a real [`App`] with in-memory secrets, the SQLite driver plus any extra drivers,
/// and an isolated home directory. Used by workflow and rendering tests.
pub struct AppHarness {
    pub app: App,
    events: mpsc::Receiver<AppEvent>,
}

impl AppHarness {
    pub fn new(root: &Path, extra_drivers: Vec<Arc<dyn DatabaseDriver>>) -> Self {
        let mut drivers: Vec<Arc<dyn DatabaseDriver>> = vec![Arc::new(SqliteDriver)];
        drivers.extend(extra_drivers);
        Self::with_driver_service(root, Arc::new(StaticDriverService::new(drivers)))
    }

    pub fn with_driver_service(root: &Path, drivers: Arc<dyn DriverService>) -> Self {
        let paths = AppPaths::rooted(root);
        if let Err(error) = paths.ensure() {
            panic!("cannot prepare test directories: {error}");
        }
        let (sender, events) = mpsc::channel(EVENT_QUEUE_CAPACITY);
        let network = ConnectionManager::new(
            TunnelManager::new(HostKeyStore::isolated(paths.known_hosts_file())),
            VpnManager::system(paths.vpn_runtime_dir()),
        );
        let context = AppContext {
            store: ProjectStore::new(paths.projects_dir()),
            paths,
            secrets: Arc::new(MemorySecretStore::default()),
            drivers,
            network,
            clipboard: Box::new(MemoryClipboard::default()),
            runtime: tokio::runtime::Handle::current(),
            events: sender,
        };
        let app = App::new(
            context,
            GlobalSettings::default(),
            ThemeCatalog::built_in(),
            Vec::new(),
        );
        Self { app, events }
    }

    /// Applies background task results until none arrive for a short while.
    pub async fn settle(&mut self) {
        while let Ok(Some(event)) = tokio::time::timeout(SETTLE_TIMEOUT, self.events.recv()).await {
            self.app.handle_event(event);
        }
    }

    pub fn key(&mut self, chord: KeyChord) {
        self.app.handle_input(InputEvent::Key(chord));
    }

    pub fn keys(&mut self, spec: &str) {
        for part in spec.split_whitespace() {
            match part.parse() {
                Ok(chord) => self.key(chord),
                Err(error) => panic!("invalid key '{part}': {error}"),
            }
        }
    }

    pub fn type_text(&mut self, text: &str) {
        for ch in text.chars() {
            let chord = match ch {
                '\n' => KeyChord::plain(KeyCode::Enter),
                ch => KeyChord::char(ch),
            };
            self.key(chord);
        }
    }

    pub fn add_sqlite(
        &mut self,
        name: &str,
        file: &Path,
        environment: Environment,
    ) -> DataSourceId {
        let mut definition = DataSourceDefinition::new(name, DriverId::from("sqlite"));
        definition.settings.file_path = Some(file.to_path_buf());
        definition.settings.create_if_missing = true;
        definition.environment = environment;
        let id = definition.id;
        self.app.save_data_source(definition, None);
        id
    }

    pub fn add_data_source(&mut self, definition: DataSourceDefinition) -> DataSourceId {
        let id = definition.id;
        self.app.save_data_source(definition, None);
        id
    }

    /// Opens a query tab for `source`, runs the whole script and waits for the results.
    pub async fn run_sql(&mut self, source: DataSourceId, sql: &str) {
        self.app.new_editor_tab(Some(source), sql);
        self.key(KeyChord::plain(KeyCode::F(9)));
        self.settle().await;
    }

    pub async fn open_key_browser(&mut self, source: DataSourceId) {
        self.app.open_key_value_tab(source, false);
        self.settle().await;
    }

    pub fn active_editor(&mut self) -> &mut EditorTab {
        match self.app.active_tab_mut() {
            Some(Tab::Editor(editor)) => editor,
            _ => panic!("expected an editor tab"),
        }
    }
}
