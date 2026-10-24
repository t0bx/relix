/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::connection::InfluxDbConnection;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, DataModel, Dialect, DriverId};
use relix_driver_api::{
    ConnectionProperty, ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities,
    DriverError, DriverMetadata,
};

pub(crate) const DEFAULT_PORT: u16 = 8181;

#[derive(Debug, Clone, Copy, Default)]
pub struct InfluxDbDriver;

impl InfluxDbDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for InfluxDbDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            DriverId::INFLUXDB,
            "InfluxDB 3",
            Some(Dialect::InfluxSql),
            ConnectionTarget::Network {
                default_port: DEFAULT_PORT,
            },
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            multiple_databases: true,
            schemas: true,
            cancellation: true,
            tls: true,
            streaming_results: true,
            ..DriverCapabilities::of_model(DataModel::TimeSeries)
        }
    }

    fn connection_properties(&self) -> Vec<ConnectionProperty> {
        vec![
            ConnectionProperty::optional(
                "database",
                "database used for queries when the database field is empty",
            ),
            ConnectionProperty::optional(
                "query_language",
                "'sql' (default) or 'influxql' for the query console",
            ),
            relix_driver_http::PATH_PREFIX,
        ]
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let connection = InfluxDbConnection::open(config).await?;
        Ok(Box::new(connection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_driver_api::DatabaseCapability;

    #[test]
    fn metadata_describes_influxdb() {
        let metadata = InfluxDbDriver::new().metadata();
        assert_eq!(metadata.id.as_str(), "influxdb");
        assert_eq!(metadata.default_port(), Some(8181));
        assert_eq!(metadata.dialect, Some(Dialect::InfluxSql));
    }

    #[test]
    fn capabilities_describe_a_read_only_time_series_database() {
        let capabilities = InfluxDbDriver::new().capabilities();
        assert!(capabilities.supports(DatabaseCapability::TimeSeriesQueries));
        assert!(capabilities.supports(DatabaseCapability::SqlExecution));
        assert!(!capabilities.table_editing && !capabilities.transactions);
    }

    #[test]
    fn properties_document_the_query_language_switch() {
        let properties = InfluxDbDriver::new().connection_properties();
        assert!(
            properties
                .iter()
                .any(|property| property.name == "query_language")
        );
        assert!(properties.iter().all(|property| !property.required));
    }
}
