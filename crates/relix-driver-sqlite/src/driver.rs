/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::blocking::run_blocking;
use crate::connection::SqliteConnection;
use crate::open::open_database;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata, with_timeout,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct SqliteDriver;

impl SqliteDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for SqliteDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            "sqlite",
            "SQLite",
            Some(Dialect::Sqlite),
            ConnectionTarget::File,
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            multiple_databases: true,
            schemas: false,
            transactions: true,
            cancellation: true,
            table_editing: true,
            routines: false,
            tls: false,
            ..DriverCapabilities::relational()
        }
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let settings = config.settings.clone();
        let timeout = settings.connect_timeout();
        let connection =
            with_timeout(timeout, run_blocking(move || open_database(&settings))).await?;
        Ok(Box::new(SqliteConnection::new(connection)))
    }
}
