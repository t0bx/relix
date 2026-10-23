/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connect::{DEFAULT_PORT, connect};
use crate::flavor::Flavor;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, DataModel, Dialect};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata,
};

macro_rules! flavored_driver {
    ($name:ident, $flavor:expr) => {
        #[derive(Debug, Clone, Copy, Default)]
        pub struct $name;

        impl $name {
            pub fn new() -> Self {
                Self
            }
        }

        #[async_trait]
        impl DatabaseDriver for $name {
            fn metadata(&self) -> DriverMetadata {
                driver_metadata($flavor)
            }

            fn capabilities(&self) -> DriverCapabilities {
                driver_capabilities()
            }

            async fn connect(
                &self,
                config: &ConnectionConfig,
            ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
                Ok(Box::new(connect($flavor, config).await?))
            }
        }
    };
}

flavored_driver!(ElasticsearchDriver, Flavor::Elasticsearch);
flavored_driver!(OpenSearchDriver, Flavor::OpenSearch);

fn driver_metadata(flavor: Flavor) -> DriverMetadata {
    DriverMetadata::built_in(
        flavor.driver_id(),
        flavor.product_name(),
        Some(Dialect::SearchRequest),
        ConnectionTarget::Network {
            default_port: DEFAULT_PORT,
        },
    )
}

fn driver_capabilities() -> DriverCapabilities {
    DriverCapabilities {
        cancellation: true,
        tls: true,
        query_execution: true,
        index_management: true,
        ..DriverCapabilities::of_model(DataModel::Search)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::DriverId;
    use relix_driver_api::DatabaseCapability;

    #[test]
    fn drivers_expose_distinct_metadata() {
        let elasticsearch = ElasticsearchDriver::new().metadata();
        let opensearch = OpenSearchDriver::new().metadata();
        assert_eq!(elasticsearch.id.as_str(), DriverId::ELASTICSEARCH);
        assert_eq!(elasticsearch.display_name, "Elasticsearch");
        assert_eq!(opensearch.id.as_str(), DriverId::OPENSEARCH);
        assert_eq!(opensearch.display_name, "OpenSearch");
        assert_eq!(opensearch.dialect, Some(Dialect::SearchRequest));
        assert_eq!(opensearch.default_port(), Some(9200));
    }

    #[test]
    fn capabilities_describe_a_search_engine() {
        let capabilities = ElasticsearchDriver::new().capabilities();
        assert_eq!(capabilities.data_model, DataModel::Search);
        assert!(capabilities.is_document());
        assert!(!capabilities.transactions && !capabilities.table_editing);
        assert!(!capabilities.multiple_databases && !capabilities.schemas);
        for capability in [
            DatabaseCapability::QueryExecution,
            DatabaseCapability::DocumentOperations,
            DatabaseCapability::IndexManagement,
            DatabaseCapability::QueryCancellation,
            DatabaseCapability::Tls,
        ] {
            assert!(capabilities.supports(capability), "{capability:?}");
        }
        assert!(!capabilities.supports(DatabaseCapability::SqlExecution));
    }
}
