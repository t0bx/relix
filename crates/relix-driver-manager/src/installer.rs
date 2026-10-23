/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::archive::extract_package;
use crate::error::ManagerError;
use crate::installed::{InstalledDriver, InstalledState, driver_dir};
use crate::manifest::ConnectorManifest;
use crate::offer::PackageOffer;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const STAGING_PREFIX: &str = ".staging-";
const DISPLACED_PREFIX: &str = ".replaced-";

/// Installs an already verified package. The archive is unpacked into a staging directory
/// inside `drivers_dir` (same file system, so the final rename is atomic), validated, moved
/// to `<id>/<version>` and only then recorded in `installed.toml`. Any failure removes the
/// staging directory and restores a displaced installation of the same version, so the
/// previous state stays intact. Older versions of the driver are removed afterwards.
pub(crate) fn install_package(
    drivers_dir: &Path,
    bytes: &[u8],
    offer: &PackageOffer,
) -> Result<InstalledDriver, ManagerError> {
    fs::create_dir_all(drivers_dir).map_err(|error| ManagerError::io(drivers_dir, error))?;
    let mut state = InstalledState::load(drivers_dir)?;
    let staging = ScratchDir::create(drivers_dir, STAGING_PREFIX, offer.id().as_str())?;
    extract_package(bytes, staging.path())?;
    let manifest = ConnectorManifest::load(staging.path())?;
    let executable = manifest.validate(offer, staging.path())?;
    make_executable(&staging.path().join(&executable))?;
    let installed = InstalledDriver {
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        api_version: manifest.api_version,
        registry: offer.registry.clone(),
        target: offer.artifact.target.clone(),
        size: offer.artifact.size,
        sha256: offer.artifact.sha256.clone(),
        executable,
        approved: true,
        installed_at: unix_time(),
        dialect: manifest.dialect,
        connection: manifest.target,
        capabilities: manifest.capabilities,
    };
    let final_dir = installed.install_dir(drivers_dir);
    let displaced = displace_existing(drivers_dir, &final_dir, offer)?;
    if let Err(error) = activate(drivers_dir, staging, &final_dir, &mut state, &installed) {
        restore(displaced.as_deref(), &final_dir);
        return Err(error);
    }
    if let Some(displaced) = displaced {
        let _ = fs::remove_dir_all(displaced);
    }
    let _ = remove_other_versions(drivers_dir, &installed);
    Ok(installed)
}

/// Removes leftovers of interrupted installations and version directories that no longer
/// belong to a recorded installation. Returns a warning for everything that could not be
/// removed.
pub(crate) fn prune_stale(drivers_dir: &Path, state: &InstalledState) -> Vec<String> {
    let mut warnings = Vec::new();
    let Ok(entries) = fs::read_dir(drivers_dir) else {
        return warnings;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(STAGING_PREFIX) || name.starts_with(DISPLACED_PREFIX) {
            remove_dir_reporting(&entry.path(), &mut warnings);
        }
    }
    for installed in &state.drivers {
        warnings.extend(remove_other_versions(drivers_dir, installed));
    }
    warnings
}

fn activate(
    drivers_dir: &Path,
    staging: ScratchDir,
    final_dir: &Path,
    state: &mut InstalledState,
    installed: &InstalledDriver,
) -> Result<(), ManagerError> {
    if let Some(parent) = final_dir.parent() {
        fs::create_dir_all(parent).map_err(|error| ManagerError::io(parent, error))?;
    }
    fs::rename(staging.path(), final_dir).map_err(|error| ManagerError::io(final_dir, error))?;
    staging.release();
    state.upsert(installed.clone());
    state.save(drivers_dir).inspect_err(|_| {
        let _ = fs::remove_dir_all(final_dir);
    })
}

fn displace_existing(
    drivers_dir: &Path,
    final_dir: &Path,
    offer: &PackageOffer,
) -> Result<Option<PathBuf>, ManagerError> {
    if !final_dir.exists() {
        return Ok(None);
    }
    let aside = unique_path(drivers_dir, DISPLACED_PREFIX, offer.id().as_str());
    fs::rename(final_dir, &aside).map_err(|error| ManagerError::io(final_dir, error))?;
    Ok(Some(aside))
}

fn restore(displaced: Option<&Path>, final_dir: &Path) {
    if let Some(displaced) = displaced {
        let _ = fs::remove_dir_all(final_dir);
        let _ = fs::rename(displaced, final_dir);
    }
}

fn remove_other_versions(drivers_dir: &Path, installed: &InstalledDriver) -> Vec<String> {
    let mut warnings = Vec::new();
    let keep = installed.install_dir(drivers_dir);
    let Ok(entries) = fs::read_dir(driver_dir(drivers_dir, &installed.id)) else {
        return warnings;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path != keep && path.is_dir() {
            remove_dir_reporting(&path, &mut warnings);
        }
    }
    warnings
}

fn remove_dir_reporting(path: &Path, warnings: &mut Vec<String>) {
    if let Err(error) = fs::remove_dir_all(path) {
        warnings.push(format!("could not remove {}: {error}", path.display()));
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), ManagerError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|error| ManagerError::io(path, error))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), ManagerError> {
    Ok(())
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

fn unique_path(parent: &Path, prefix: &str, label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        "{prefix}{label}-{}-{}-{sequence}",
        std::process::id(),
        unix_time()
    ))
}

struct ScratchDir {
    path: PathBuf,
    armed: bool,
}

impl ScratchDir {
    fn create(parent: &Path, prefix: &str, label: &str) -> Result<Self, ManagerError> {
        let path = unique_path(parent, prefix, label);
        fs::create_dir(&path).map_err(|error| ManagerError::io(&path, error))?;
        Ok(Self { path, armed: true })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn release(mut self) {
        self.armed = false;
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
