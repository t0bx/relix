/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::PostgresConnection;
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
                Ok(Box::new(PostgresConnection::open($flavor, config).await?))
            }
        }
    };
}

flavored_driver!(PostgresDriver, Flavor::Postgres);
flavored_driver!(CockroachDbDriver, Flavor::CockroachDb);
flavored_driver!(YugabyteDbDriver, Flavor::YugabyteDb);
flavored_driver!(TimescaleDbDriver, Flavor::TimescaleDb);

fn driver_metadata(flavor: Flavor) -> DriverMetadata {
    DriverMetadata::built_in(
        flavor.driver_id(),
        flavor.product_name(),
        Some(Dialect::Postgres),
        ConnectionTarget::Network {
            default_port: flavor.default_port(),
        },
    )
}

fn driver_capabilities() -> DriverCapabilities {
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

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::DriverId;

    #[test]
    fn metadata_describes_each_engine() {
        let postgres = PostgresDriver::new().metadata();
        assert_eq!(postgres.id.as_str(), DriverId::POSTGRES);
        assert_eq!(postgres.default_port(), Some(5432));
        assert_eq!(postgres.dialect, Some(Dialect::Postgres));
        let cockroach = CockroachDbDriver::new().metadata();
        assert_eq!(cockroach.id.as_str(), DriverId::COCKROACHDB);
        assert_eq!(cockroach.display_name, "CockroachDB");
        assert_eq!(cockroach.default_port(), Some(26257));
        let yugabyte = YugabyteDbDriver::new().metadata();
        assert_eq!(yugabyte.id.as_str(), DriverId::YUGABYTEDB);
        assert_eq!(yugabyte.default_port(), Some(5433));
        let timescale = TimescaleDbDriver::new().metadata();
        assert_eq!(timescale.id.as_str(), DriverId::TIMESCALEDB);
        assert_eq!(timescale.default_port(), Some(5432));
        assert_eq!(timescale.dialect, Some(Dialect::Postgres));
    }

    #[test]
    fn capabilities_enable_relational_features() {
        let capabilities = CockroachDbDriver::new().capabilities();
        assert!(capabilities.is_relational());
        assert!(capabilities.multiple_databases && capabilities.schemas);
        assert!(capabilities.cancellation && capabilities.routines && capabilities.tls);
    }
}
