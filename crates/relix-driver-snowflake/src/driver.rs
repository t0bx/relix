/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::SnowflakeConnection;
use crate::settings::DEFAULT_PORT;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect, DriverId};
use relix_driver_api::{
    ConnectionProperty, ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities,
    DriverError, DriverMetadata, with_timeout,
};

/// Snowflake over the SQL API v2. All requests go to the single account host
/// (`<account>.snowflakecomputing.com:443`), result partitions included, so the target
/// is a network endpoint: it can be reached through an SSH tunnel (e.g. a bastion that
/// is allowed by a network policy) with TLS still verified against the account host.
#[derive(Debug, Clone, Copy, Default)]
pub struct SnowflakeDriver;

impl SnowflakeDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DatabaseDriver for SnowflakeDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            DriverId::SNOWFLAKE,
            "Snowflake",
            Some(Dialect::Snowflake),
            ConnectionTarget::Network {
                default_port: DEFAULT_PORT,
            },
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            multiple_databases: true,
            schemas: true,
            transactions: false,
            cancellation: true,
            table_editing: false,
            routines: true,
            tls: true,
            ..DriverCapabilities::relational()
        }
    }

    fn connection_properties(&self) -> Vec<ConnectionProperty> {
        vec![
            ConnectionProperty::optional(
                "account",
                "account identifier, e.g. myorg-myaccount (otherwise taken from the host)",
            ),
            ConnectionProperty::optional(
                "authenticator",
                "snowflake_jwt (default, key pair), oauth or programmatic_access_token",
            ),
            ConnectionProperty::optional(
                "private_key_file",
                "PKCS#8 private key for key-pair authentication; passphrase in the password field",
            ),
            ConnectionProperty::optional("warehouse", "virtual warehouse"),
            ConnectionProperty::optional("role", "role to use"),
            ConnectionProperty::optional("schema", "default schema"),
        ]
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let timeout = config.settings.connect_timeout();
        let connection = with_timeout(timeout, SnowflakeConnection::open(config)).await?;
        Ok(Box::new(connection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_describes_a_tunnelable_https_endpoint() {
        let metadata = SnowflakeDriver::new().metadata();
        assert_eq!(metadata.id.as_str(), "snowflake");
        assert_eq!(metadata.default_port(), Some(443));
        assert!(metadata.target.supports_tunnel());
        assert_eq!(metadata.dialect, Some(Dialect::Snowflake));
    }

    #[test]
    fn grid_editing_and_transactions_are_disabled() {
        let capabilities = SnowflakeDriver::new().capabilities();
        assert!(capabilities.is_relational() && capabilities.cancellation);
        assert!(!capabilities.table_editing && !capabilities.transactions);
    }
}
