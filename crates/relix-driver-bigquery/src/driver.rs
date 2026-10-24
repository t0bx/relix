/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::BigQueryConnection;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect, DriverId};
use relix_driver_api::{
    ConnectionProperty, ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities,
    DriverError, DriverMetadata, with_timeout,
};

/// Google BigQuery over the REST API v2. Requests go to Google's API front ends and
/// token endpoints rather than a single database host, so the target is a cloud service:
/// SSH port forwarding does not apply, a VPN or the system proxy does.
#[derive(Debug, Clone, Copy, Default)]
pub struct BigQueryDriver;

impl BigQueryDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for BigQueryDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            DriverId::BIGQUERY,
            "BigQuery",
            Some(Dialect::BigQuery),
            ConnectionTarget::Cloud,
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            multiple_databases: true,
            schemas: true,
            transactions: true,
            cancellation: true,
            table_editing: false,
            routines: true,
            tls: true,
            ..DriverCapabilities::relational()
        }
    }

    fn connection_properties(&self) -> Vec<ConnectionProperty> {
        vec![
            ConnectionProperty::optional(
                "project_id",
                "Google Cloud project (otherwise the database field or the credentials project)",
            ),
            ConnectionProperty::optional(
                "credentials_file",
                "service account key file (otherwise Application Default Credentials)",
            ),
            ConnectionProperty::optional(
                "auth",
                "adc (default), service_account, or none together with endpoint",
            ),
            ConnectionProperty::optional("location", "job location, e.g. EU"),
            ConnectionProperty::optional("dataset", "default dataset"),
            ConnectionProperty::optional(
                "additional_projects",
                "comma-separated projects to list in the explorer",
            ),
            ConnectionProperty::optional(
                "maximum_bytes_billed",
                "refuse queries that would bill more bytes",
            ),
            ConnectionProperty::optional(
                "endpoint",
                "API base URL override, e.g. a local emulator",
            ),
        ]
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let timeout = config.settings.connect_timeout();
        let connection = with_timeout(timeout, BigQueryConnection::open(config)).await?;
        Ok(Box::new(connection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_describes_a_cloud_service() {
        let metadata = BigQueryDriver::new().metadata();
        assert_eq!(metadata.id.as_str(), "bigquery");
        assert_eq!(metadata.target, ConnectionTarget::Cloud);
        assert!(!metadata.target.supports_tunnel());
        assert_eq!(metadata.default_port(), None);
    }

    #[test]
    fn projects_and_datasets_map_to_databases_and_schemas() {
        let capabilities = BigQueryDriver::new().capabilities();
        assert!(capabilities.multiple_databases && capabilities.schemas);
        assert!(capabilities.cancellation && !capabilities.table_editing);
    }
}
