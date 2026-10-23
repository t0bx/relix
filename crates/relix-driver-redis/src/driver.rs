/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connect::open_connection;
use crate::flavor::Flavor;
use async_trait::async_trait;
use relix_core::ConnectionConfig;
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata,
};

pub const DEFAULT_PORT: u16 = 6379;

macro_rules! flavored_driver {
    ($name:ident, $flavor:expr) => {
        #[derive(Debug, Clone, Copy, Default)]
        pub struct $name;

        impl $name {
            pub fn new() -> Self {
                Self
            }
        }

        #[async_trait]
        impl DatabaseDriver for $name {
            fn metadata(&self) -> DriverMetadata {
                driver_metadata($flavor)
            }

            fn capabilities(&self) -> DriverCapabilities {
                driver_capabilities()
            }

            async fn connect(
                &self,
                config: &ConnectionConfig,
            ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
                Ok(Box::new(open_connection($flavor, config).await?))
            }
        }
    };
}

flavored_driver!(RedisDriver, Flavor::Redis);
flavored_driver!(ValkeyDriver, Flavor::Valkey);

fn driver_metadata(flavor: Flavor) -> DriverMetadata {
    DriverMetadata::built_in(
        flavor.driver_id(),
        flavor.product_name(),
        None,
        ConnectionTarget::Network {
            default_port: DEFAULT_PORT,
        },
    )
}

fn driver_capabilities() -> DriverCapabilities {
    DriverCapabilities {
        tls: true,
        multiple_databases: true,
        ..DriverCapabilities::key_value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::{DataModel, DriverId};

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

    #[test]
    fn valkey_has_its_own_identity() {
        let metadata = ValkeyDriver::new().metadata();
        assert_eq!(metadata.id.as_str(), DriverId::VALKEY);
        assert_eq!(metadata.display_name, "Valkey");
        assert_eq!(metadata.default_port(), Some(6379));
        assert_eq!(
            ValkeyDriver::new().capabilities(),
            RedisDriver::new().capabilities()
        );
    }
}
