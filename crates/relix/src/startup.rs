/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::cli::Cli;
use crate::driver_service::ManagerDriverService;
use crate::logging;
use anyhow::Context;
use relix_app::app::EVENT_QUEUE_CAPACITY;
use relix_app::{App, AppContext};
use relix_config::{AppPaths, GlobalSettings, ThemeCatalog};
use relix_connections::{ConnectionManager, HostKeyStore, TunnelManager, VpnManager};
use relix_driver_manager::{
    DownloadProgress, DriverManager, HttpsFetcher, ManagerError, PackageFetcher, RegistrySource,
};
use relix_project::{FallbackSecretStore, ProjectStore};
use relix_tui::Osc52Clipboard;
use relix_tui::terminal::TerminalGuard;
use std::sync::Arc;
use tokio::sync::mpsc;

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let paths = AppPaths::discover().context("cannot determine configuration directories")?;
    if cli.print_paths {
        println!("config: {}", paths.config_dir.display());
        println!("data:   {}", paths.data_dir.display());
        println!("logs:   {}", paths.logs_dir().display());
        return Ok(());
    }
    paths
        .ensure()
        .context("cannot create application directories")?;
    let _log_guard = logging::init(&paths.logs_dir());
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting relix");
    let load =
        GlobalSettings::load_or_recover(&paths.settings_file()).context("cannot load settings")?;
    let mut settings = load.settings;
    let mut warnings = load.warnings;
    if let Some(theme) = cli.theme {
        settings.theme = theme;
    }
    let mouse = settings.ui.mouse && !cli.no_mouse;
    let themes = ThemeCatalog::load(&paths.themes_dir());
    warnings.extend(themes.warnings.iter().cloned());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("relix-worker")
        .build()
        .context("cannot start the async runtime")?;
    let manager = build_driver_manager(&paths, &settings, &mut warnings);
    let (sender, receiver) = mpsc::channel(EVENT_QUEUE_CAPACITY);
    let network = ConnectionManager::new(
        TunnelManager::new(HostKeyStore::new(paths.known_hosts_file())),
        VpnManager::system(paths.vpn_runtime_dir()),
    );
    let exit_network = network.clone();
    let context = AppContext {
        store: ProjectStore::new(paths.projects_dir()),
        paths,
        secrets: Arc::new(FallbackSecretStore::default()),
        drivers: Arc::new(ManagerDriverService::new(manager)),
        network,
        clipboard: Box::new(Osc52Clipboard::default()),
        runtime: runtime.handle().clone(),
        events: sender,
    };
    let mut app = App::new(context, settings, themes, warnings);
    app.startup(cli.project.as_deref());
    let result = runtime.block_on(async {
        let mut guard = TerminalGuard::enter(mouse).context("cannot initialise the terminal")?;
        relix_tui::run(&mut app, receiver, &mut guard)
            .await
            .context("terminal error")
    });
    runtime.block_on(async {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        exit_network.shutdown().await;
    });
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    tracing::info!("relix stopped");
    result
}

/// Registers the compiled-in drivers and every approved, compatible external connector.
fn build_driver_manager(
    paths: &AppPaths,
    settings: &GlobalSettings,
    warnings: &mut Vec<String>,
) -> Arc<DriverManager> {
    let registries = settings
        .drivers
        .registries
        .iter()
        .map(RegistrySource::from)
        .collect();
    let (fetcher, offline): (Arc<dyn PackageFetcher>, bool) = match HttpsFetcher::new() {
        Ok(fetcher) => (Arc::new(fetcher), settings.drivers.offline),
        Err(error) => {
            warnings.push(format!("driver downloads disabled: {error}"));
            (Arc::new(OfflineFetcher), true)
        }
    };
    let manager = DriverManager::new(paths.drivers_dir(), registries, offline, fetcher);
    manager.register_built_in(Arc::new(relix_driver_postgres::PostgresDriver));
    manager.register_built_in(Arc::new(relix_driver_mysql::MySqlDriver));
    manager.register_built_in(Arc::new(relix_driver_mysql::MariaDbDriver));
    manager.register_built_in(Arc::new(relix_driver_sqlite::SqliteDriver));
    manager.register_built_in(Arc::new(relix_driver_redis::RedisDriver));
    manager.register_built_in(Arc::new(relix_driver_search::ElasticsearchDriver));
    manager.register_built_in(Arc::new(relix_driver_search::OpenSearchDriver));
    warnings.extend(manager.load_installed());
    Arc::new(manager)
}

/// Stand-in transport used when HTTPS could not be initialised; the manager is then
/// put into offline mode, so this is never asked to fetch anything.
struct OfflineFetcher;

#[async_trait::async_trait]
impl PackageFetcher for OfflineFetcher {
    async fn fetch_index(&self, _url: &str, _max_bytes: u64) -> Result<Vec<u8>, ManagerError> {
        Err(ManagerError::Offline)
    }

    async fn download(
        &self,
        _url: &str,
        _max_bytes: u64,
        _progress: DownloadProgress<'_>,
    ) -> Result<Vec<u8>, ManagerError> {
        Err(ManagerError::Offline)
    }
}
