/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connect::open_connection;
use async_trait::async_trait;
use relix_core::ConnectionConfig;
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata,
};

pub const DEFAULT_PORT: u16 = 6379;

#[derive(Debug, Clone, Copy, Default)]
pub struct RedisDriver;

impl RedisDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for RedisDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            "redis",
            "Redis",
            None,
            ConnectionTarget::Network {
                default_port: DEFAULT_PORT,
            },
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            tls: true,
            multiple_databases: true,
            ..DriverCapabilities::key_value()
        }
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let connection = open_connection(config).await?;
        Ok(Box::new(connection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::DataModel;

    #[test]
    fn metadata_describes_a_network_key_value_driver() {
        let driver = RedisDriver::new();
        let metadata = driver.metadata();
        assert_eq!(metadata.id.as_str(), "redis");
        assert_eq!(metadata.default_port(), Some(6379));
        assert!(metadata.dialect.is_none());
        let capabilities = driver.capabilities();
        assert_eq!(capabilities.data_model, DataModel::KeyValue);
        assert!(capabilities.tls);
        assert!(capabilities.multiple_databases);
        assert!(!capabilities.transactions);
    }
}
