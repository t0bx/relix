/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::{App, Focus};
use crate::events::AppEvent;
use crate::notifications::Level;
use crate::overlay::{ConfirmState, Overlay, PendingAction, PromptAction};
use crate::requests::RequestId;
use crate::services::InstallOffer;
use crate::tab::Tab;
use crate::workspace::TabId;
use relix_core::{ConnectionConfig, DataSourceDefinition, DataSourceId, Dialect, DriverId, Secret};
use relix_driver_api::{
    DatabaseConnection, DriverCapabilities, DriverError, ErrorKind, ServerInfo,
};
use relix_project::{SecretKey, SecretStore};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Deferred {
    ExpandNode(String),
    RunEditor { tab: TabId, script: bool },
    LoadTable(TabId),
    ScanKeys(TabId),
}

pub struct ConnectionState {
    pub status: ConnectionStatus,
    pub handle: Option<Arc<dyn DatabaseConnection>>,
    pub server: Option<ServerInfo>,
    pub(crate) request: Option<RequestId>,
    pub(crate) deferred: Vec<Deferred>,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self {
            status: ConnectionStatus::Disconnected,
            handle: None,
            server: None,
            request: None,
            deferred: Vec::new(),
        }
    }
}

impl App {
    pub fn data_source(&self, id: DataSourceId) -> Option<&DataSourceDefinition> {
        self.project.as_ref()?.data_source(id)
    }

    pub fn connection_status(&self, id: DataSourceId) -> ConnectionStatus {
        self.connections
            .get(&id)
            .map_or(ConnectionStatus::Disconnected, |state| state.status.clone())
    }

    pub(crate) fn connection(&self, id: DataSourceId) -> Option<Arc<dyn DatabaseConnection>> {
        self.connections
            .get(&id)
            .and_then(|state| state.handle.clone())
    }

    pub fn capabilities(&self, id: DataSourceId) -> Option<DriverCapabilities> {
        let definition = self.data_source(id)?;
        self.context
            .drivers
            .driver(&definition.driver)
            .map(|driver| driver.capabilities())
    }

    pub fn dialect(&self, id: DataSourceId) -> Dialect {
        self.data_source(id)
            .and_then(|definition| self.context.drivers.driver(&definition.driver))
            .and_then(|driver| driver.metadata().dialect)
            .unwrap_or(Dialect::Generic)
    }

    pub fn driver_name(&self, id: &DriverId) -> String {
        self.context
            .drivers
            .driver(id)
            .map_or_else(|| id.to_string(), |driver| driver.metadata().display_name)
    }

    /// Returns the live connection, or starts connecting and remembers `deferred` so the
    /// action resumes automatically once the connection is established.
    pub(crate) fn connection_or_defer(
        &mut self,
        source: DataSourceId,
        deferred: Deferred,
    ) -> Option<Arc<dyn DatabaseConnection>> {
        if let Some(handle) = self.connection(source) {
            return Some(handle);
        }
        let state = self.connections.entry(source).or_default();
        if !state.deferred.contains(&deferred) {
            state.deferred.push(deferred);
        }
        if state.status != ConnectionStatus::Connecting {
            self.connect(source);
        }
        None
    }

    pub(crate) fn connect(&mut self, source: DataSourceId) {
        let Some(definition) = self.data_source(source).cloned() else {
            return;
        };
        let Some(driver) = self.context.drivers.driver(&definition.driver) else {
            self.offer_driver_install(source, definition.driver.clone());
            return;
        };
        let session_password = self.session_passwords.get(&source).cloned();
        if session_password.is_none()
            && !definition.settings.save_password
            && self.requires_password(&definition)
        {
            self.prompt_password(source);
            return;
        }
        let request = self.next_request();
        let state = self.connections.entry(source).or_default();
        state.status = ConnectionStatus::Connecting;
        state.request = Some(request);
        let secrets = self.context.secrets.clone();
        let key = self.secret_key(source);
        let save_password = definition.settings.save_password;
        self.status = format!("connecting to {}…", definition.name);
        self.spawn(async move {
            let password = match (session_password, key) {
                (Some(password), _) => Some(password),
                (None, Some(key)) if save_password => load_secret(secrets, key).await,
                _ => None,
            };
            let config = ConnectionConfig::new(definition.settings, password);
            let result = driver.connect(&config).await.map(Arc::from);
            AppEvent::Connected {
                source,
                request,
                result,
            }
        });
    }

    fn requires_password(&self, definition: &DataSourceDefinition) -> bool {
        self.context
            .drivers
            .driver(&definition.driver)
            .is_some_and(|driver| {
                matches!(
                    driver.metadata().target,
                    relix_driver_api::ConnectionTarget::Network { .. }
                )
            })
    }

    pub(crate) fn secret_key(&self, source: DataSourceId) -> Option<SecretKey> {
        self.project
            .as_ref()
            .map(|project| SecretKey::for_data_source(project.id, source))
    }

    fn prompt_password(&mut self, source: DataSourceId) {
        let name = self
            .data_source(source)
            .map(|definition| definition.name.clone())
            .unwrap_or_default();
        self.overlays
            .push(Overlay::Prompt(crate::overlay::PromptState {
                title: format!("Password for {name}"),
                hint: "used for this session only".into(),
                input: crate::input::LineInput::default(),
                masked: true,
                action: PromptAction::Password { source },
            }));
    }

    pub(crate) fn submit_password(&mut self, source: DataSourceId, password: String) {
        self.session_passwords.insert(source, Secret::new(password));
        self.connect(source);
    }

    pub(crate) fn on_connected(
        &mut self,
        source: DataSourceId,
        request: RequestId,
        result: Result<Arc<dyn DatabaseConnection>, DriverError>,
    ) {
        let name = self
            .data_source(source)
            .map(|definition| definition.name.clone())
            .unwrap_or_default();
        let Some(state) = self.connections.get_mut(&source) else {
            return;
        };
        if state.request != Some(request) {
            if let Ok(stale) = result {
                self.context.runtime.spawn(async move {
                    let _ = stale.close().await;
                });
            }
            return;
        }
        match result {
            Ok(handle) => {
                state.server = Some(handle.server_info());
                state.handle = Some(handle);
                state.status = ConnectionStatus::Connected;
                let deferred = std::mem::take(&mut state.deferred);
                let server = state
                    .server
                    .as_ref()
                    .map(|server| format!("{} {}", server.product, server.version))
                    .unwrap_or_default();
                self.status = format!("connected to {name} ({server})");
                self.notify(Level::Success, format!("connected to {name}"));
                self.load_catalog(source);
                self.resume_deferred(deferred);
                self.resume_explorer_expansions();
            }
            Err(error) => {
                let deferred_tabs: Vec<TabId> = state
                    .deferred
                    .drain(..)
                    .filter_map(|deferred| match deferred {
                        Deferred::RunEditor { tab, .. } => Some(tab),
                        _ => None,
                    })
                    .collect();
                state.status = ConnectionStatus::Failed(error.to_string());
                for tab in deferred_tabs {
                    if let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab) {
                        editor.running = None;
                    }
                }
                self.explorer_connection_failed(source, &error);
                if error.kind == ErrorKind::Authentication {
                    self.session_passwords.remove(&source);
                    self.prompt_password(source);
                }
                self.status = format!("connection to {name} failed");
                self.notify(Level::Error, format!("{name}: {error}"));
            }
        }
    }

    fn resume_deferred(&mut self, deferred: Vec<Deferred>) {
        for action in deferred {
            match action {
                Deferred::ExpandNode(key) => self.load_explorer_key(&key),
                Deferred::RunEditor { tab, script } => {
                    if self.workspace.tab(tab).is_some() {
                        self.workspace.activate_tab(tab);
                        if let Some(Tab::Editor(editor)) = self.workspace.tab_mut(tab) {
                            editor.running = None;
                        }
                        self.run_editor(script, false);
                    }
                }
                Deferred::LoadTable(tab) => self.load_table(tab),
                Deferred::ScanKeys(tab) => self.start_key_scan(tab, true),
            }
        }
    }

    /// Marks a connection as lost when an operation fails at the transport level, so the
    /// UI stops pretending it is connected and the next action reconnects. Statements are
    /// never retried automatically because their outcome is unknown.
    pub(crate) fn note_driver_error(&mut self, source: DataSourceId, error: &DriverError) {
        if !error.kind.is_connection_loss() {
            return;
        }
        let name = self
            .data_source(source)
            .map(|definition| definition.name.clone())
            .unwrap_or_default();
        if let Some(state) = self.connections.get_mut(&source) {
            state.status = ConnectionStatus::Failed("connection lost".into());
            if let Some(handle) = state.handle.take() {
                self.context.runtime.spawn(async move {
                    let _ = handle.close().await;
                });
            }
        }
        self.notify(
            Level::Warning,
            format!("connection to {name} was lost — it reconnects on the next action"),
        );
    }

    pub(crate) fn disconnect(&mut self, source: DataSourceId) {
        if let Some(state) = self.connections.remove(&source)
            && let Some(handle) = state.handle
        {
            self.context.runtime.spawn(async move {
                let _ = handle.close().await;
            });
        }
        self.catalogs.remove(&source);
        if let Some(root) = self.explorer.root_for(source) {
            self.explorer.reset(root);
        }
    }

    pub(crate) fn disconnect_all(&mut self) {
        let sources: Vec<DataSourceId> = self.connections.keys().copied().collect();
        for source in sources {
            self.disconnect(source);
        }
    }

    pub(crate) fn toggle_connection_for_context(&mut self) {
        let source = match self.focus {
            Focus::Sidebar => self.selected_explorer_source(),
            Focus::Workspace => self.active_tab().and_then(Tab::data_source),
        };
        let Some(source) = source else {
            self.notify(Level::Info, "select a data source first");
            return;
        };
        match self.connection_status(source) {
            ConnectionStatus::Connected | ConnectionStatus::Connecting => {
                self.disconnect(source);
                self.status = "disconnected".into();
            }
            _ => self.connect(source),
        }
    }

    fn offer_driver_install(&mut self, source: DataSourceId, driver: DriverId) {
        let service = self.context.drivers.clone();
        self.notify(
            Level::Info,
            format!("driver '{driver}' is not installed, checking registries…"),
        );
        self.spawn(async move {
            let result = service.find_offer(&driver).await;
            AppEvent::DriverOffer {
                source,
                driver,
                result,
            }
        });
    }

    pub(crate) fn on_driver_offer(
        &mut self,
        source: DataSourceId,
        driver: DriverId,
        result: Result<Option<InstallOffer>, String>,
    ) {
        match result {
            Ok(Some(offer)) => self.confirm_driver_install(offer, Some(source)),
            Ok(None) => self.notify(
                Level::Error,
                format!("no installable package found for driver '{driver}'"),
            ),
            Err(error) => self.notify(Level::Error, format!("driver lookup failed: {error}")),
        }
    }

    pub(crate) fn confirm_driver_install(
        &mut self,
        offer: InstallOffer,
        retry: Option<DataSourceId>,
    ) {
        self.overlays.push(Overlay::Confirm(ConfirmState {
            title: "Install database driver?".into(),
            lines: vec![
                format!("Driver:  {} v{}", offer.name, offer.version),
                format!("Source:  {}", offer.source),
                format!("Size:    {}", human_size(offer.size_bytes)),
                offer.description.clone(),
                String::new(),
                "The package is verified (SHA-256 + signature) before installation and".into(),
                "runs as a separate connector process.".into(),
            ],
            confirm_label: "Install".into(),
            danger: false,
            action: PendingAction::InstallDriver { offer, retry },
        }));
    }

    pub(crate) fn test_connection(
        &mut self,
        definition: DataSourceDefinition,
        password: Option<Secret>,
    ) {
        let Some(driver) = self.context.drivers.driver(&definition.driver) else {
            self.notify(
                Level::Error,
                format!("driver '{}' is not available", definition.driver),
            );
            return;
        };
        let secrets = self.context.secrets.clone();
        let key = self.secret_key(definition.id);
        self.spawn(async move {
            let password = match (password, key) {
                (Some(password), _) => Some(password),
                (None, Some(key)) => load_secret(secrets, key).await,
                (None, None) => None,
            };
            let config = ConnectionConfig::new(definition.settings, password);
            let result = async {
                let connection = driver.connect(&config).await?;
                connection.ping().await?;
                let info = connection.server_info();
                let _ = connection.close().await;
                Ok(info)
            }
            .await;
            AppEvent::ConnectionTested { result }
        });
    }
}

async fn load_secret(secrets: Arc<dyn SecretStore>, key: SecretKey) -> Option<Secret> {
    tokio::task::spawn_blocking(move || secrets.get(&key))
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::human_size;

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(8_808_038), "8.4 MB");
    }
}
