/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connect_target::DEFAULT_PORT;
use crate::connection::OracleConnection;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect, DriverId};
use relix_driver_api::{
    ConnectionProperty, ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities,
    DriverError, DriverMetadata, with_timeout,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct OracleDriver;

impl OracleDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for OracleDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            DriverId::ORACLE,
            "Oracle Database",
            Some(Dialect::Oracle),
            ConnectionTarget::Network {
                default_port: DEFAULT_PORT,
            },
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            multiple_databases: false,
            schemas: true,
            transactions: true,
            cancellation: true,
            table_editing: true,
            routines: true,
            tls: false,
            ..DriverCapabilities::relational()
        }
    }

    fn connection_properties(&self) -> Vec<ConnectionProperty> {
        vec![ConnectionProperty::optional(
            "client_lib_dir",
            "directory of the Oracle Instant Client (needed on macOS)",
        )]
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let connection = with_timeout(
            config.settings.connect_timeout(),
            OracleConnection::open(config),
        )
        .await?;
        Ok(Box::new(connection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_describes_a_network_engine() {
        let metadata = OracleDriver::new().metadata();
        assert_eq!(metadata.id.as_str(), DriverId::ORACLE);
        assert_eq!(metadata.dialect, Some(Dialect::Oracle));
        assert_eq!(metadata.default_port(), Some(1521));
    }

    #[test]
    fn capabilities_describe_schemas_without_tls() {
        let capabilities = OracleDriver::new().capabilities();
        assert!(capabilities.is_relational());
        assert!(capabilities.schemas && !capabilities.multiple_databases);
        assert!(capabilities.cancellation && capabilities.routines);
        assert!(!capabilities.tls);
    }
}
