/*
 * @author t0bx
 * @since 23.10.2026
 */

use async_trait::async_trait;
use relix_core::{Dialect, DriverId};
use relix_driver_api::{ConnectionTarget, DatabaseDriver, DriverCapabilities};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverDescriptor {
    pub id: DriverId,
    pub name: String,
    pub version: String,
    pub origin: String,
    pub status: String,
    pub target: ConnectionTarget,
    pub dialect: Option<Dialect>,
    pub capabilities: DriverCapabilities,
    pub default_port: Option<u16>,
}

impl DriverDescriptor {
    pub fn test(
        id: &str,
        name: &str,
        target: ConnectionTarget,
        capabilities: DriverCapabilities,
    ) -> Self {
        Self {
            id: DriverId::new(id),
            name: name.to_owned(),
            version: "test".to_owned(),
            origin: "built-in".to_owned(),
            status: "Ready".to_owned(),
            default_port: match target {
                ConnectionTarget::Network { default_port } => Some(default_port),
                ConnectionTarget::File | ConnectionTarget::Cloud => None,
            },
            target,
            dialect: None,
            capabilities,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOffer {
    pub id: DriverId,
    pub name: String,
    pub version: String,
    pub source: String,
    pub size_bytes: u64,
    pub description: String,
}

/// The application's view of driver management. Implemented on top of the driver
/// manager in the executable, and by in-memory fakes in tests.
#[async_trait]
pub trait DriverService: Send + Sync {
    fn driver(&self, id: &DriverId) -> Option<Arc<dyn DatabaseDriver>>;

    fn descriptors(&self) -> Vec<DriverDescriptor>;

    async fn find_offer(&self, id: &DriverId) -> Result<Option<InstallOffer>, String>;

    async fn install(&self, id: &DriverId) -> Result<String, String>;

    async fn updates(&self) -> Result<Vec<InstallOffer>, String>;

    fn remove(&self, id: &DriverId) -> Result<(), String>;
}

pub trait Clipboard: Send {
    fn set_text(&mut self, text: &str) -> Result<(), String>;

    fn text(&mut self) -> Option<String>;
}

#[derive(Debug, Default)]
pub struct MemoryClipboard {
    content: Option<String>,
}

impl Clipboard for MemoryClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), String> {
        self.content = Some(text.to_owned());
        Ok(())
    }

    fn text(&mut self) -> Option<String> {
        self.content.clone()
    }
}

/// A driver service over a fixed set of compiled-in drivers without package management.
pub struct StaticDriverService {
    drivers: Vec<Arc<dyn DatabaseDriver>>,
}

impl StaticDriverService {
    pub fn new(drivers: Vec<Arc<dyn DatabaseDriver>>) -> Self {
        Self { drivers }
    }
}

pub fn describe_driver(driver: &dyn DatabaseDriver, status: &str) -> DriverDescriptor {
    let metadata = driver.metadata();
    DriverDescriptor {
        default_port: metadata.default_port(),
        id: metadata.id,
        name: metadata.display_name,
        version: metadata.version,
        origin: match metadata.origin {
            relix_driver_api::DriverOrigin::BuiltIn => "built-in".to_owned(),
            relix_driver_api::DriverOrigin::External { package } => format!("external ({package})"),
        },
        status: status.to_owned(),
        target: metadata.target,
        dialect: metadata.dialect,
        capabilities: driver.capabilities(),
    }
}

#[async_trait]
impl DriverService for StaticDriverService {
    fn driver(&self, id: &DriverId) -> Option<Arc<dyn DatabaseDriver>> {
        self.drivers
            .iter()
            .find(|driver| &driver.metadata().id == id)
            .cloned()
    }

    fn descriptors(&self) -> Vec<DriverDescriptor> {
        self.drivers
            .iter()
            .map(|driver| describe_driver(driver.as_ref(), "Ready"))
            .collect()
    }

    async fn find_offer(&self, _id: &DriverId) -> Result<Option<InstallOffer>, String> {
        Ok(None)
    }

    async fn install(&self, id: &DriverId) -> Result<String, String> {
        Err(format!("no package source configured for '{id}'"))
    }

    async fn updates(&self) -> Result<Vec<InstallOffer>, String> {
        Ok(Vec::new())
    }

    fn remove(&self, id: &DriverId) -> Result<(), String> {
        Err(format!("'{id}' is built in"))
    }
}
