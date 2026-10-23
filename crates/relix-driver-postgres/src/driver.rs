/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::PostgresConnection;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata,
};

pub(crate) const DEFAULT_PORT: u16 = 5432;

#[derive(Debug, Clone, Copy, Default)]
pub struct PostgresDriver;

impl PostgresDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for PostgresDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            "postgres",
            "PostgreSQL",
            Some(Dialect::Postgres),
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
        let connection = PostgresConnection::open(config).await?;
        Ok(Box::new(connection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_describes_postgres() {
        let metadata = PostgresDriver::new().metadata();
        assert_eq!(metadata.id.as_str(), "postgres");
        assert_eq!(metadata.default_port(), Some(5432));
        assert_eq!(metadata.dialect, Some(Dialect::Postgres));
    }

    #[test]
    fn capabilities_enable_relational_features() {
        let capabilities = PostgresDriver::new().capabilities();
        assert!(capabilities.is_relational());
        assert!(capabilities.multiple_databases && capabilities.schemas);
        assert!(capabilities.cancellation && capabilities.routines && capabilities.tls);
    }
}
