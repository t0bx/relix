/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connect::open_connection;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, DataModel, Dialect, DriverId};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata,
};

pub const DEFAULT_PORT: u16 = 27017;

#[derive(Debug, Clone, Copy, Default)]
pub struct MongoDbDriver;

impl MongoDbDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for MongoDbDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            DriverId::MONGODB,
            "MongoDB",
            Some(Dialect::MongoCommand),
            ConnectionTarget::Network {
                default_port: DEFAULT_PORT,
            },
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            multiple_databases: true,
            cancellation: true,
            tls: true,
            index_management: true,
            ..DriverCapabilities::of_model(DataModel::Document)
        }
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let connection = open_connection(config).await?;
        Ok(Box::new(connection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_driver_api::DatabaseCapability;

    #[test]
    fn metadata_describes_a_network_document_driver() {
        let driver = MongoDbDriver::new();
        let metadata = driver.metadata();
        assert_eq!(metadata.id.as_str(), "mongodb");
        assert_eq!(metadata.default_port(), Some(27017));
        assert_eq!(metadata.dialect, Some(Dialect::MongoCommand));
        let capabilities = driver.capabilities();
        assert!(capabilities.is_document());
        assert!(capabilities.supports(DatabaseCapability::QueryExecution));
        assert!(capabilities.supports(DatabaseCapability::IndexManagement));
        assert!(capabilities.supports(DatabaseCapability::QueryCancellation));
        assert!(!capabilities.supports(DatabaseCapability::Transactions));
        assert!(!capabilities.supports(DatabaseCapability::TableEditing));
    }
}
