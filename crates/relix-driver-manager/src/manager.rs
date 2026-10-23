/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::compatibility::{check_api_version, check_target, current_target};
use crate::entry::{DriverEntry, DriverStatus, IndexRefresh, InstallProgress};
use crate::error::ManagerError;
use crate::external::ExternalDriver;
use crate::fetcher::{PackageFetcher, ensure_https};
use crate::index::{MAX_INDEX_BYTES, PackageIndex};
use crate::installed::{InstalledDriver, InstalledState, driver_dir, usability_problem};
use crate::installer::{install_package, prune_stale};
use crate::offer::{CachedIndex, PackageOffer, best_offer};
use crate::registry::RegistrySource;
use crate::verification::{MAX_PACKAGE_BYTES, parse_public_key, verify_package};
use relix_core::DriverId;
use relix_driver_api::DatabaseDriver;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub type ProgressCallback<'a> = &'a (dyn Fn(InstallProgress) + Send + Sync);

#[derive(Default)]
struct ManagerState {
    built_in: BTreeMap<DriverId, Arc<dyn DatabaseDriver>>,
    external: BTreeMap<DriverId, Arc<dyn DatabaseDriver>>,
    installed: Vec<InstalledDriver>,
    problems: BTreeMap<DriverId, String>,
    indexes: Vec<CachedIndex>,
}

pub struct DriverManager {
    drivers_dir: PathBuf,
    registries: Vec<RegistrySource>,
    offline: AtomicBool,
    target: String,
    fetcher: Arc<dyn PackageFetcher>,
    state: RwLock<ManagerState>,
    install_lock: tokio::sync::Mutex<()>,
}

impl DriverManager {
    pub fn new(
        drivers_dir: impl Into<PathBuf>,
        registries: Vec<RegistrySource>,
        offline: bool,
        fetcher: Arc<dyn PackageFetcher>,
    ) -> Self {
        Self {
            drivers_dir: drivers_dir.into(),
            registries,
            offline: AtomicBool::new(offline),
            target: current_target(),
            fetcher,
            state: RwLock::new(ManagerState::default()),
            install_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = target.into();
        self
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn drivers_dir(&self) -> &Path {
        &self.drivers_dir
    }

    pub fn registries(&self) -> &[RegistrySource] {
        &self.registries
    }

    pub fn is_offline(&self) -> bool {
        self.offline.load(Ordering::Relaxed)
    }

    pub fn set_offline(&self, offline: bool) {
        self.offline.store(offline, Ordering::Relaxed);
    }

    pub fn register_built_in(&self, driver: Arc<dyn DatabaseDriver>) {
        let id = driver.metadata().id;
        let mut state = self.write();
        state.external.remove(&id);
        state.built_in.insert(id, driver);
    }

    /// Reads `installed.toml` and registers every approved, compatible external driver.
    /// Call it after the built-in drivers are registered. Returns human-readable warnings
    /// for installations that cannot be used and for leftovers that could not be cleaned.
    pub fn load_installed(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let recorded = InstalledState::load(&self.drivers_dir).unwrap_or_else(|error| {
            warnings.push(format!("cannot read installed drivers: {error}"));
            InstalledState::default()
        });
        if let Ok(_installing) = self.install_lock.try_lock() {
            warnings.extend(prune_stale(&self.drivers_dir, &recorded));
        }
        let mut state = self.write();
        state.external.clear();
        state.problems.clear();
        for installed in &recorded.drivers {
            let problem = if state.built_in.contains_key(&installed.id) {
                Some("its id is already used by a built-in driver".to_owned())
            } else {
                usability_problem(installed, &self.drivers_dir, &self.target)
            };
            match problem {
                Some(reason) => {
                    warnings.push(format!(
                        "driver {} {} is unavailable: {reason}",
                        installed.id, installed.version
                    ));
                    state.problems.insert(installed.id.clone(), reason);
                }
                None => {
                    let driver = ExternalDriver::from_installed(installed, &self.drivers_dir);
                    state
                        .external
                        .insert(installed.id.clone(), Arc::new(driver));
                }
            }
        }
        state.installed = recorded.drivers;
        warnings
    }

    pub fn driver(&self, id: &DriverId) -> Option<Arc<dyn DatabaseDriver>> {
        let state = self.read();
        state
            .built_in
            .get(id)
            .or_else(|| state.external.get(id))
            .cloned()
    }

    pub fn installed(&self) -> Vec<InstalledDriver> {
        self.read().installed.clone()
    }

    pub fn entries(&self) -> Vec<DriverEntry> {
        let state = self.read();
        let built_in = state.built_in.values().map(|driver| DriverEntry {
            metadata: driver.metadata(),
            capabilities: driver.capabilities(),
            status: DriverStatus::Ready,
            installed: None,
        });
        let external = state.installed.iter().map(|installed| {
            let update = best_offer(&state.indexes, &installed.id, &self.target)
                .filter(|offer| offer.release.version > installed.version);
            let status = match (state.problems.get(&installed.id), update) {
                (Some(reason), _) => DriverStatus::Incompatible(reason.clone()),
                (None, Some(offer)) => DriverStatus::UpdateAvailable(offer.release.version),
                (None, None) => DriverStatus::Ready,
            };
            DriverEntry {
                metadata: installed.metadata(),
                capabilities: installed.capabilities,
                status,
                installed: Some(installed.clone()),
            }
        });
        built_in.chain(external).collect()
    }

    /// Downloads the index of every configured registry and replaces the cached copy of
    /// each one that succeeded. Fails only when offline or when every registry failed;
    /// individual failures are listed in the result.
    pub async fn refresh_index(&self) -> Result<IndexRefresh, ManagerError> {
        if self.is_offline() {
            return Err(ManagerError::Offline);
        }
        let mut refreshed = Vec::new();
        let mut errors = Vec::new();
        for registry in &self.registries {
            match self.fetch_registry(registry).await {
                Ok(index) => refreshed.push(CachedIndex {
                    registry: registry.name.clone(),
                    index,
                }),
                Err(error) => errors.push(ManagerError::Registry {
                    registry: registry.name.clone(),
                    message: error.to_string(),
                }),
            }
        }
        if refreshed.is_empty() && !errors.is_empty() {
            return Err(errors.swap_remove(0));
        }
        let releases = refreshed
            .iter()
            .map(|cached| cached.index.releases.len())
            .sum();
        let mut state = self.write();
        for cached in refreshed {
            state
                .indexes
                .retain(|existing| existing.registry != cached.registry);
            state.indexes.push(cached);
        }
        let order = |name: &str| {
            self.registries
                .iter()
                .position(|registry| registry.name == name)
        };
        state.indexes.sort_by_key(|cached| order(&cached.registry));
        Ok(IndexRefresh {
            releases,
            failures: errors.iter().map(ToString::to_string).collect(),
        })
    }

    pub fn offer_for(&self, id: &DriverId) -> Option<PackageOffer> {
        let state = self.read();
        if state.built_in.contains_key(id) {
            return None;
        }
        best_offer(&state.indexes, id, &self.target)
    }

    pub fn outdated(&self) -> Vec<(InstalledDriver, PackageOffer)> {
        let state = self.read();
        state
            .installed
            .iter()
            .filter_map(|installed| {
                best_offer(&state.indexes, &installed.id, &self.target)
                    .filter(|offer| offer.release.version > installed.version)
                    .map(|offer| (installed.clone(), offer))
            })
            .collect()
    }

    /// Downloads, verifies and installs `offer`, then registers the driver. Calling this
    /// is the user's approval to run the package, so the UI must have shown the offer
    /// (name, version, registry, size) and asked first. Installs are serialized.
    pub async fn install(
        &self,
        offer: &PackageOffer,
        progress: ProgressCallback<'_>,
    ) -> Result<InstalledDriver, ManagerError> {
        let _installing = self.install_lock.lock().await;
        if self.is_offline() {
            return Err(ManagerError::Offline);
        }
        self.check_offer(offer)?;
        let key = parse_public_key(&self.registry(&offer.registry)?.public_key)?;
        let total = offer.artifact.size;
        progress(InstallProgress::Downloading { received: 0, total });
        let report = |received| progress(InstallProgress::Downloading { received, total });
        let bytes = self
            .fetcher
            .download(&offer.artifact.url, total, &report)
            .await?;
        progress(InstallProgress::Verifying);
        verify_package(&bytes, &offer.release, &offer.artifact, &key)?;
        progress(InstallProgress::Installing);
        let drivers_dir = self.drivers_dir.clone();
        let verified_offer = offer.clone();
        let installed = tokio::task::spawn_blocking(move || {
            install_package(&drivers_dir, &bytes, &verified_offer)
        })
        .await
        .map_err(|error| ManagerError::Internal(error.to_string()))??;
        self.register_installed(&installed);
        Ok(installed)
    }

    /// Unregisters an installed external driver and deletes its files. Connections that
    /// are already open keep their connector process until they are closed.
    pub fn remove(&self, id: &DriverId) -> Result<(), ManagerError> {
        let _installing = self
            .install_lock
            .try_lock()
            .map_err(|_| ManagerError::Busy)?;
        if self.read().built_in.contains_key(id) {
            return Err(ManagerError::BuiltIn(id.clone()));
        }
        let mut recorded = InstalledState::load(&self.drivers_dir)?;
        recorded
            .remove(id)
            .ok_or_else(|| ManagerError::NotInstalled(id.clone()))?;
        recorded.save(&self.drivers_dir)?;
        {
            let mut state = self.write();
            state.external.remove(id);
            state.problems.remove(id);
            state.installed.retain(|installed| &installed.id != id);
        }
        let directory = driver_dir(&self.drivers_dir, id);
        match fs::remove_dir_all(&directory) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(ManagerError::io(&directory, error))
            }
            _ => Ok(()),
        }
    }

    async fn fetch_registry(
        &self,
        registry: &RegistrySource,
    ) -> Result<PackageIndex, ManagerError> {
        ensure_https(&registry.url)?;
        parse_public_key(&registry.public_key)?;
        let bytes = self
            .fetcher
            .fetch_index(&registry.url, MAX_INDEX_BYTES)
            .await?;
        PackageIndex::parse(&bytes)
    }

    fn check_offer(&self, offer: &PackageOffer) -> Result<(), ManagerError> {
        if self.read().built_in.contains_key(offer.id()) {
            return Err(ManagerError::BuiltIn(offer.id().clone()));
        }
        check_api_version(offer.release.api_version).map_err(ManagerError::Incompatible)?;
        check_target(&offer.artifact.target, &self.target).map_err(ManagerError::Incompatible)?;
        ensure_https(&offer.artifact.url)?;
        if offer.artifact.size > MAX_PACKAGE_BYTES {
            return Err(ManagerError::TooLarge {
                limit: MAX_PACKAGE_BYTES,
            });
        }
        Ok(())
    }

    fn registry(&self, name: &str) -> Result<&RegistrySource, ManagerError> {
        self.registries
            .iter()
            .find(|registry| registry.name == name)
            .ok_or_else(|| ManagerError::UnknownRegistry(name.to_owned()))
    }

    fn register_installed(&self, installed: &InstalledDriver) {
        let driver = ExternalDriver::from_installed(installed, &self.drivers_dir);
        let mut state = self.write();
        state.problems.remove(&installed.id);
        state
            .installed
            .retain(|existing| existing.id != installed.id);
        state.installed.push(installed.clone());
        state
            .installed
            .sort_by(|left, right| left.id.cmp(&right.id));
        state
            .external
            .insert(installed.id.clone(), Arc::new(driver));
    }

    fn read(&self) -> RwLockReadGuard<'_, ManagerState> {
        self.state.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, ManagerState> {
        self.state.write().unwrap_or_else(PoisonError::into_inner)
    }
}
