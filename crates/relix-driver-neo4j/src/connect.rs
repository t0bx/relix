/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::driver::DEFAULT_PORT;
use crate::errors::map_error;
use neo4rs::{ConfigBuilder, Graph};
use relix_core::ConnectionConfig;
use relix_driver_api::DriverError;
use relix_tls_relay::{DriverEndpoint, open_endpoint};
use std::time::Duration;

const FETCH_SIZE: usize = 500;
const MAX_CONNECTIONS: usize = 4;
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

pub(crate) struct LiveGraph {
    pub(crate) graph: Graph,
    _endpoint: DriverEndpoint,
}

/// Builds the neo4rs connection pool. neo4rs applies its connection timeout to every
/// read as well, so the pool gets the configured query timeout (or a day) for reads and
/// the connect timeout is enforced by the caller around the first round trip instead.
pub(crate) async fn open_graph(config: &ConnectionConfig) -> Result<LiveGraph, DriverError> {
    let endpoint = open_endpoint(config, DEFAULT_PORT).await?;
    let mut builder = ConfigBuilder::default()
        .uri(format!("bolt://{}", endpoint.address()))
        .user(config.settings.username.clone().unwrap_or_default())
        .password(
            config
                .password
                .as_ref()
                .map(|password| password.expose().to_owned())
                .unwrap_or_default(),
        )
        .fetch_size(FETCH_SIZE)
        .max_connections(MAX_CONNECTIONS)
        .connection_timeout(
            config
                .settings
                .query_timeout()
                .unwrap_or(DEFAULT_READ_TIMEOUT),
        );
    if let Some(database) = configured_database(config) {
        builder = builder.db(database);
    }
    let graph = Graph::connect(builder.build().map_err(map_error)?).map_err(map_error)?;
    Ok(LiveGraph {
        graph,
        _endpoint: endpoint,
    })
}

pub(crate) fn configured_database(config: &ConnectionConfig) -> Option<&str> {
    config
        .settings
        .database
        .as_deref()
        .map(str::trim)
        .filter(|database| !database.is_empty())
}
