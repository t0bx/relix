/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::DynamoConnection;
use crate::errors::map_sdk_error;
use crate::sdk_config::{ENDPOINT_PROPERTY, load_sdk_config};
use aws_sdk_dynamodb::Client;
use relix_core::ConnectionConfig;
use relix_driver_api::{DriverError, ServerInfo, with_timeout};

const API_VERSION: &str = "2012-08-10";

/// Resolves configuration and credentials, then lists one table so missing
/// credentials, a wrong region or an unreachable endpoint fail while connecting.
pub async fn open_connection(config: &ConnectionConfig) -> Result<DynamoConnection, DriverError> {
    with_timeout(config.settings.connect_timeout(), async {
        let sdk_config = load_sdk_config(config).await?;
        let region = sdk_config
            .region()
            .map(|region| region.as_ref().to_owned())
            .unwrap_or_default();
        let client = Client::new(&sdk_config);
        client
            .list_tables()
            .limit(1)
            .send()
            .await
            .map_err(map_sdk_error)?;
        let product = match config.settings.property(ENDPOINT_PROPERTY) {
            Some(endpoint) => format!("DynamoDB ({endpoint})"),
            None => "Amazon DynamoDB".to_owned(),
        };
        Ok(DynamoConnection::new(
            client,
            region,
            ServerInfo {
                product,
                version: API_VERSION.to_owned(),
            },
        ))
    })
    .await
}
