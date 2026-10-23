/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::CassandraConnection;
use crate::flavor::Flavor;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, DataModel, Dialect};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata,
};

pub(crate) const DEFAULT_PORT: u16 = 9042;

#[derive(Debug, Clone, Copy, Default)]
pub struct CassandraDriver;

impl CassandraDriver {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ScyllaDbDriver;

impl ScyllaDbDriver {
    pub fn new() -> Self {
        Self
    }
}

fn metadata(flavor: Flavor) -> DriverMetadata {
    DriverMetadata::built_in(
        flavor.driver_id(),
        flavor.display_name(),
        Some(Dialect::Cql),
        ConnectionTarget::Network {
            default_port: DEFAULT_PORT,
        },
    )
}

fn capabilities() -> DriverCapabilities {
    DriverCapabilities {
        multiple_databases: true,
        cancellation: true,
        table_editing: true,
        tls: true,
        streaming_results: true,
        ..DriverCapabilities::of_model(DataModel::WideColumn)
    }
}

async fn connect(config: &ConnectionConfig) -> Result<Box<dyn DatabaseConnection>, DriverError> {
    let connection = CassandraConnection::open(config).await?;
    Ok(Box::new(connection))
}

#[async_trait]
impl DatabaseDriver for CassandraDriver {
    fn metadata(&self) -> DriverMetadata {
        metadata(Flavor::Cassandra)
    }

    fn capabilities(&self) -> DriverCapabilities {
        capabilities()
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        connect(config).await
    }
}

#[async_trait]
impl DatabaseDriver for ScyllaDbDriver {
    fn metadata(&self) -> DriverMetadata {
        metadata(Flavor::ScyllaDb)
    }

    fn capabilities(&self) -> DriverCapabilities {
        capabilities()
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        connect(config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_driver_api::DatabaseCapability;

    #[test]
    fn both_flavors_speak_cql_on_the_default_port() {
        let cassandra = CassandraDriver::new().metadata();
        let scylla = ScyllaDbDriver::new().metadata();
        assert_eq!(cassandra.id.as_str(), "cassandra");
        assert_eq!(scylla.id.as_str(), "scylladb");
        for metadata in [cassandra, scylla] {
            assert_eq!(metadata.default_port(), Some(9042));
            assert_eq!(metadata.dialect, Some(Dialect::Cql));
        }
    }

    #[test]
    fn capabilities_describe_a_wide_column_store_without_transactions() {
        let capabilities = CassandraDriver::new().capabilities();
        assert_eq!(capabilities.data_model, DataModel::WideColumn);
        assert!(capabilities.multiple_databases && !capabilities.schemas);
        assert!(!capabilities.transactions);
        assert!(capabilities.supports(DatabaseCapability::SqlExecution));
        assert!(capabilities.supports(DatabaseCapability::TableEditing));
        assert!(!capabilities.supports(DatabaseCapability::Routines));
    }
}
