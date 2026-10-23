/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::capabilities::DriverCapabilities;
use crate::connection::DatabaseConnection;
use crate::error::DriverError;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect, DriverId};
use serde::{Deserialize, Serialize};

pub const DRIVER_API_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DriverOrigin {
    BuiltIn,
    External { package: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectionTarget {
    Network { default_port: u16 },
    File,
    Cloud,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverMetadata {
    pub id: DriverId,
    pub display_name: String,
    pub version: String,
    pub api_version: u32,
    pub origin: DriverOrigin,
    pub dialect: Option<Dialect>,
    pub target: ConnectionTarget,
}

impl DriverMetadata {
    pub fn built_in(
        id: &str,
        display_name: &str,
        dialect: Option<Dialect>,
        target: ConnectionTarget,
    ) -> Self {
        Self {
            id: DriverId::new(id),
            display_name: display_name.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            api_version: DRIVER_API_VERSION,
            origin: DriverOrigin::BuiltIn,
            dialect,
            target,
        }
    }

    pub fn default_port(&self) -> Option<u16> {
        match self.target {
            ConnectionTarget::Network { default_port } => Some(default_port),
            ConnectionTarget::File | ConnectionTarget::Cloud => None,
        }
    }
}

impl ConnectionTarget {
    pub fn is_network(self) -> bool {
        matches!(self, Self::Network { .. })
    }

    pub fn supports_tunnel(self) -> bool {
        self.is_network()
    }
}

#[async_trait]
pub trait DatabaseDriver: Send + Sync {
    fn metadata(&self) -> DriverMetadata;

    fn capabilities(&self) -> DriverCapabilities;

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError>;
}
