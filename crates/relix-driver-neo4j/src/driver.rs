/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::connection::Neo4jConnection;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, DataModel, Dialect, DriverId};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata,
};

pub(crate) const DEFAULT_PORT: u16 = 7687;

#[derive(Debug, Clone, Copy, Default)]
pub struct Neo4jDriver;

impl Neo4jDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for Neo4jDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            DriverId::NEO4J,
            "Neo4j",
            Some(Dialect::Cypher),
            ConnectionTarget::Network {
                default_port: DEFAULT_PORT,
            },
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            multiple_databases: true,
            transactions: true,
            cancellation: true,
            table_editing: true,
            tls: true,
            streaming_results: true,
            ..DriverCapabilities::of_model(DataModel::Graph)
        }
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let connection = Neo4jConnection::open(config).await?;
        Ok(Box::new(connection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_driver_api::DatabaseCapability;

    #[test]
    fn metadata_describes_neo4j() {
        let metadata = Neo4jDriver::new().metadata();
        assert_eq!(metadata.id.as_str(), "neo4j");
        assert_eq!(metadata.default_port(), Some(7687));
        assert_eq!(metadata.dialect, Some(Dialect::Cypher));
    }

    #[test]
    fn capabilities_describe_a_transactional_graph_database() {
        let capabilities = Neo4jDriver::new().capabilities();
        assert!(capabilities.supports(DatabaseCapability::GraphQueries));
        assert!(capabilities.supports(DatabaseCapability::Transactions));
        assert!(!capabilities.supports(DatabaseCapability::SqlExecution));
        assert!(!capabilities.schemas);
    }
}
