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

#[derive(Debug, Clone, Copy, Default)]
pub struct DynamoDbDriver;

impl DynamoDbDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for DynamoDbDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            DriverId::DYNAMODB,
            "Amazon DynamoDB",
            Some(Dialect::PartiQl),
            ConnectionTarget::Cloud,
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
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
    fn metadata_describes_a_cloud_document_driver() {
        let driver = DynamoDbDriver::new();
        let metadata = driver.metadata();
        assert_eq!(metadata.id.as_str(), "dynamodb");
        assert_eq!(metadata.target, ConnectionTarget::Cloud);
        assert_eq!(metadata.default_port(), None);
        assert_eq!(metadata.dialect, Some(Dialect::PartiQl));
        let capabilities = driver.capabilities();
        assert!(capabilities.is_document());
        assert!(!capabilities.multiple_databases);
        assert!(capabilities.supports(DatabaseCapability::QueryExecution));
        assert!(capabilities.supports(DatabaseCapability::IndexManagement));
        assert!(!capabilities.supports(DatabaseCapability::Transactions));
    }
}
