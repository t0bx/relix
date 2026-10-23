/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::archive::safe_relative_path;
use crate::error::ManagerError;
use crate::offer::PackageOffer;
use relix_core::{Dialect, DriverId};
use relix_driver_api::{ConnectionTarget, DriverCapabilities};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MANIFEST_FILE: &str = "connector.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorManifest {
    pub id: DriverId,
    pub name: String,
    pub version: Version,
    pub api_version: u32,
    pub executable: PathBuf,
    #[serde(default)]
    pub dialect: Option<Dialect>,
    pub target: ConnectionTarget,
    #[serde(default = "DriverCapabilities::relational")]
    pub capabilities: DriverCapabilities,
}

impl ConnectorManifest {
    pub fn load(package_dir: &Path) -> Result<Self, ManagerError> {
        relix_config::files::read_toml(&package_dir.join(MANIFEST_FILE))
            .map_err(|error| ManagerError::InvalidPackage(error.to_string()))?
            .ok_or_else(|| ManagerError::InvalidPackage(format!("{MANIFEST_FILE} is missing")))
    }

    /// Ensures the unpacked package is the one that was signed and offered, that it is a
    /// relational connector, and that its executable is a regular file inside the package.
    /// Returns the executable path relative to the package directory.
    pub fn validate(
        &self,
        offer: &PackageOffer,
        package_dir: &Path,
    ) -> Result<PathBuf, ManagerError> {
        let release = &offer.release;
        if self.id != release.id
            || self.version != release.version
            || self.api_version != release.api_version
        {
            return Err(ManagerError::InvalidPackage(format!(
                "{MANIFEST_FILE} describes {} {} (API {}), but {} {} (API {}) was offered",
                self.id,
                self.version,
                self.api_version,
                release.id,
                release.version,
                release.api_version
            )));
        }
        if !self.capabilities.is_relational() {
            return Err(ManagerError::Incompatible(
                "only relational connectors are supported by connector protocol version 1"
                    .to_owned(),
            ));
        }
        let executable = safe_relative_path(&self.executable)?.ok_or_else(|| {
            ManagerError::InvalidPackage("the manifest names no executable".to_owned())
        })?;
        if !package_dir.join(&executable).is_file() {
            return Err(ManagerError::InvalidPackage(format!(
                "executable {} is missing from the package",
                executable.display()
            )));
        }
        Ok(executable)
    }
}
