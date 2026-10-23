/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::MsSqlConnection;
use crate::connector::DEFAULT_PORT;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect, DriverId};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata, with_timeout,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct MsSqlDriver;

impl MsSqlDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for MsSqlDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            DriverId::MSSQL,
            "SQL Server",
            Some(Dialect::MsSql),
            ConnectionTarget::Network {
                default_port: DEFAULT_PORT,
            },
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            multiple_databases: true,
            schemas: true,
            transactions: true,
            cancellation: true,
            table_editing: true,
            routines: true,
            tls: true,
            ..DriverCapabilities::relational()
        }
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let connection = with_timeout(
            config.settings.connect_timeout(),
            MsSqlConnection::open(config),
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
        let metadata = MsSqlDriver::new().metadata();
        assert_eq!(metadata.id.as_str(), DriverId::MSSQL);
        assert_eq!(metadata.dialect, Some(Dialect::MsSql));
        assert_eq!(metadata.default_port(), Some(1433));
    }

    #[test]
    fn capabilities_describe_a_multi_database_engine_with_schemas() {
        let capabilities = MsSqlDriver::new().capabilities();
        assert!(capabilities.is_relational());
        assert!(capabilities.multiple_databases && capabilities.schemas);
        assert!(capabilities.cancellation && capabilities.routines && capabilities.tls);
    }
}
