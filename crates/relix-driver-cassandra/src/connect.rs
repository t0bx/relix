/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::driver::DEFAULT_PORT;
use crate::errors::map_session_error;
use crate::routing::SingleEndpoint;
use relix_core::ConnectionConfig;
use relix_driver_api::{DriverError, ErrorKind, with_timeout};
use relix_tls_relay::{DriverEndpoint, open_endpoint};
use scylla::client::PoolSize;
use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;
use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::sync::Arc;

pub(crate) struct LiveSession {
    pub(crate) session: Session,
    _endpoint: DriverEndpoint,
}

pub(crate) async fn open_session(config: &ConnectionConfig) -> Result<LiveSession, DriverError> {
    let timeout = config.settings.connect_timeout();
    with_timeout(timeout, async {
        let endpoint = open_endpoint(config, DEFAULT_PORT).await?;
        let address = resolve(&endpoint).await?;
        let session = builder(config, address)
            .build()
            .await
            .map_err(|error| map_session_error(&error))?;
        if let Some(keyspace) = configured_keyspace(config) {
            session
                .use_keyspace(keyspace, true)
                .await
                .map_err(|error| {
                    DriverError::configuration(format!("cannot use keyspace '{keyspace}'"))
                        .with_detail(error.to_string())
                })?;
        }
        Ok(LiveSession {
            session,
            _endpoint: endpoint,
        })
    })
    .await
}

pub(crate) fn configured_keyspace(config: &ConnectionConfig) -> Option<&str> {
    config
        .settings
        .database
        .as_deref()
        .map(str::trim)
        .filter(|keyspace| !keyspace.is_empty())
}

fn builder(config: &ConnectionConfig, address: SocketAddr) -> SessionBuilder {
    let builder = SessionBuilder::new()
        .known_node_addr(address)
        .address_translator(Arc::new(SingleEndpoint(address)))
        .pool_size(PoolSize::PerHost(NonZeroUsize::MIN))
        .disallow_shard_aware_port(true)
        .connection_timeout(config.settings.connect_timeout())
        .fetch_schema_metadata(false);
    match config.settings.username.as_deref().map(str::trim) {
        Some(user) if !user.is_empty() => builder.user(
            user,
            config
                .password
                .as_ref()
                .map(|password| password.expose().to_owned())
                .unwrap_or_default(),
        ),
        _ => builder,
    }
}

async fn resolve(endpoint: &DriverEndpoint) -> Result<SocketAddr, DriverError> {
    let mut addresses = tokio::net::lookup_host((endpoint.host(), endpoint.port()))
        .await
        .map_err(|error| {
            DriverError::new(
                ErrorKind::Connection,
                format!("cannot resolve {}: {error}", endpoint.host()),
            )
        })?;
    addresses.next().ok_or_else(|| {
        DriverError::connection(format!("{} has no network address", endpoint.host()))
    })
}
