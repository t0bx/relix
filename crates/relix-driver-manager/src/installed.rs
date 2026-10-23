/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ManagerError;
use relix_core::{Dialect, DriverId};
use relix_driver_api::{ConnectionTarget, DriverCapabilities, DriverMetadata, DriverOrigin};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const INSTALLED_FILE: &str = "installed.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledDriver {
    pub id: DriverId,
    pub name: String,
    pub version: Version,
    pub api_version: u32,
    pub registry: String,
    pub target: String,
    pub size: u64,
    pub sha256: String,
    pub executable: PathBuf,
    pub approved: bool,
    pub installed_at: u64,
    #[serde(default)]
    pub dialect: Option<Dialect>,
    pub connection: ConnectionTarget,
    pub capabilities: DriverCapabilities,
}

impl InstalledDriver {
    pub fn install_dir(&self, drivers_dir: &Path) -> PathBuf {
        version_dir(drivers_dir, &self.id, &self.version)
    }

    pub fn executable_path(&self, drivers_dir: &Path) -> PathBuf {
        self.install_dir(drivers_dir).join(&self.executable)
    }

    pub fn package_label(&self) -> String {
        format!("{}:{}@{}", self.registry, self.id, self.version)
    }

    pub fn metadata(&self) -> DriverMetadata {
        DriverMetadata {
            id: self.id.clone(),
            display_name: self.name.clone(),
            version: self.version.to_string(),
            api_version: self.api_version,
            origin: DriverOrigin::External {
                package: self.package_label(),
            },
            dialect: self.dialect,
            target: self.connection,
        }
    }
}

pub fn driver_dir(drivers_dir: &Path, id: &DriverId) -> PathBuf {
    drivers_dir.join(id.as_str())
}

pub fn version_dir(drivers_dir: &Path, id: &DriverId, version: &Version) -> PathBuf {
    driver_dir(drivers_dir, id).join(version.to_string())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InstalledState {
    #[serde(default)]
    pub(crate) drivers: Vec<InstalledDriver>,
}

impl InstalledState {
    pub(crate) fn load(drivers_dir: &Path) -> Result<Self, ManagerError> {
        Ok(relix_config::files::read_toml(&drivers_dir.join(INSTALLED_FILE))?.unwrap_or_default())
    }

    pub(crate) fn save(&self, drivers_dir: &Path) -> Result<(), ManagerError> {
        relix_config::files::write_toml(&drivers_dir.join(INSTALLED_FILE), self)?;
        Ok(())
    }

    pub(crate) fn upsert(&mut self, installed: InstalledDriver) {
        self.drivers.retain(|driver| driver.id != installed.id);
        self.drivers.push(installed);
        self.drivers.sort_by(|left, right| left.id.cmp(&right.id));
    }

    pub(crate) fn remove(&mut self, id: &DriverId) -> Option<InstalledDriver> {
        let position = self.drivers.iter().position(|driver| &driver.id == id)?;
        Some(self.drivers.remove(position))
    }
}

pub(crate) fn usability_problem(
    installed: &InstalledDriver,
    drivers_dir: &Path,
    host_target: &str,
) -> Option<String> {
    if !installed.approved {
        return Some("the package was never approved; reinstall it to approve it".to_owned());
    }
    if let Err(reason) = crate::compatibility::check_api_version(installed.api_version) {
        return Some(reason);
    }
    if let Err(reason) = crate::compatibility::check_target(&installed.target, host_target) {
        return Some(reason);
    }
    if !installed.capabilities.is_relational() {
        return Some("only relational connectors are supported".to_owned());
    }
    let executable = installed.executable_path(drivers_dir);
    if !executable.is_file() {
        return Some(format!("executable {} is missing", executable.display()));
    }
    None
}
