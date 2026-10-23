/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::{ClickHouseConnection, DEFAULT_HTTP_PORT};
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect, DriverId};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata, with_timeout,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct ClickHouseDriver;

impl ClickHouseDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for ClickHouseDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            DriverId::CLICKHOUSE,
            "ClickHouse",
            Some(Dialect::ClickHouse),
            ConnectionTarget::Network {
                default_port: DEFAULT_HTTP_PORT,
            },
        )
    }

    /// Grid editing stays off: `ALTER TABLE … UPDATE/DELETE` are asynchronous mutations
    /// and lightweight deletes report no affected rows, so neither the single-row guard
    /// nor an all-or-nothing change set can be guaranteed.
    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            multiple_databases: true,
            schemas: false,
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
        let connection = with_timeout(timeout, ClickHouseConnection::open(config)).await?;
        Ok(Box::new(connection))
    }
}
