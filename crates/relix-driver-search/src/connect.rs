/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::auth::http_auth;
use crate::client::SearchClient;
use crate::cluster::root_info;
use crate::connection::SearchConnection;
use crate::flavor::Flavor;
use crate::options::SearchOptions;
use relix_core::ConnectionConfig;
use relix_driver_api::{DriverError, with_timeout};
use relix_driver_http::{EndpointOptions, HttpEndpoint};

pub const DEFAULT_PORT: u16 = 9200;

pub async fn connect(
    flavor: Flavor,
    config: &ConnectionConfig,
) -> Result<SearchConnection, DriverError> {
    with_timeout(config.settings.connect_timeout(), establish(flavor, config)).await
}

async fn establish(
    flavor: Flavor,
    config: &ConnectionConfig,
) -> Result<SearchConnection, DriverError> {
    let options = SearchOptions::from_settings(&config.settings)?;
    let endpoint = HttpEndpoint::connect(
        config,
        EndpointOptions {
            default_port: DEFAULT_PORT,
            probe_path: "/".to_owned(),
            auth: http_auth(config)?,
            request_timeout: config.settings.query_timeout(),
        },
    )
    .await?;
    let client = SearchClient::new(endpoint);
    let root = root_info(&client).await?;
    let server_info = flavor.server_info(&root);
    Ok(SearchConnection::new(client, root, server_info, options))
}
