/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::{DEFAULT_PORT, TrinoConnection};
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect, DriverId};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata, with_timeout,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct TrinoDriver;

impl TrinoDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for TrinoDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            DriverId::TRINO,
            "Trino",
            Some(Dialect::Trino),
            ConnectionTarget::Network {
                default_port: DEFAULT_PORT,
            },
        )
    }

    /// Catalogs are presented as databases. Grid editing and transactions stay off:
    /// most connectors support neither row-level updates guarded by original values nor
    /// multi-statement transactions.
    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            multiple_databases: true,
            schemas: true,
            transactions: false,
            cancellation: true,
            table_editing: false,
            routines: false,
            tls: true,
            streaming_results: true,
            ..DriverCapabilities::relational()
        }
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let timeout = config.settings.connect_timeout();
        let connection = with_timeout(timeout, TrinoConnection::open(config)).await?;
        Ok(Box::new(connection))
    }
}
