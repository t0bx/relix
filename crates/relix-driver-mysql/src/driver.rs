/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connect::{DEFAULT_PORT, connect};
use crate::flavor::Flavor;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata,
};

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
                Ok(Box::new(connect($flavor, config).await?))
            }
        }
    };
}

flavored_driver!(MySqlDriver, Flavor::MySql);
flavored_driver!(MariaDbDriver, Flavor::MariaDb);

fn driver_metadata(flavor: Flavor) -> DriverMetadata {
    DriverMetadata::built_in(
        flavor.driver_id(),
        flavor.product_name(),
        Some(Dialect::MySql),
        ConnectionTarget::Network {
            default_port: DEFAULT_PORT,
        },
    )
}

fn driver_capabilities() -> DriverCapabilities {
    DriverCapabilities {
        multiple_databases: true,
        schemas: false,
        transactions: true,
        cancellation: true,
        table_editing: true,
        routines: true,
        tls: true,
        ..DriverCapabilities::relational()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::DriverId;

    #[test]
    fn drivers_expose_distinct_metadata() {
        let mysql = MySqlDriver::new().metadata();
        let mariadb = MariaDbDriver::new().metadata();
        assert_eq!(mysql.id.as_str(), DriverId::MYSQL);
        assert_eq!(mysql.display_name, "MySQL");
        assert_eq!(mariadb.id.as_str(), DriverId::MARIADB);
        assert_eq!(mariadb.display_name, "MariaDB");
        assert_eq!(mariadb.dialect, Some(Dialect::MySql));
        assert_eq!(mariadb.default_port(), Some(3306));
    }

    #[test]
    fn capabilities_describe_a_relational_multi_database_engine() {
        let capabilities = MariaDbDriver::new().capabilities();
        assert!(capabilities.is_relational());
        assert!(capabilities.multiple_databases && !capabilities.schemas);
        assert!(capabilities.cancellation && capabilities.routines && capabilities.tls);
    }
}
