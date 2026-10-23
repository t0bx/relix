/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::installed::InstalledDriver;
use relix_driver_api::{DriverCapabilities, DriverMetadata};
use semver::Version;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverStatus {
    Ready,
    Incompatible(String),
    UpdateAvailable(Version),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverEntry {
    pub metadata: DriverMetadata,
    pub capabilities: DriverCapabilities,
    pub status: DriverStatus,
    pub installed: Option<InstalledDriver>,
}

impl DriverEntry {
    pub fn is_built_in(&self) -> bool {
        self.installed.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallProgress {
    Downloading { received: u64, total: u64 },
    Verifying,
    Installing,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexRefresh {
    pub releases: usize,
    pub failures: Vec<String>,
}
