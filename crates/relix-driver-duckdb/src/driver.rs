/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::blocking::run_blocking;
use crate::connection::DuckDbConnection;
use crate::open::{ACCESS_MODE, open_database};
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect, DriverId};
use relix_driver_api::{
    ConnectionProperty, ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities,
    DriverError, DriverMetadata, with_timeout,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct DuckDbDriver;

impl DuckDbDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for DuckDbDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            DriverId::DUCKDB,
            "DuckDB",
            Some(Dialect::DuckDb),
            ConnectionTarget::File,
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
            tls: false,
            streaming_results: true,
            ..DriverCapabilities::relational()
        }
    }

    fn connection_properties(&self) -> Vec<ConnectionProperty> {
        vec![ACCESS_MODE]
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let settings = config.settings.clone();
        let timeout = settings.connect_timeout();
        let opened = with_timeout(timeout, run_blocking(move || open_database(&settings))).await?;
        Ok(Box::new(DuckDbConnection::new(opened)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_the_properties_it_reads() {
        let names: Vec<_> = DuckDbDriver::new()
            .connection_properties()
            .iter()
            .map(|property| property.name)
            .collect();
        assert_eq!(names, ["access_mode"]);
    }
}
