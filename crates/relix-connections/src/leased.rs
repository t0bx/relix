/*
 * @author t0bx
 * @since 23.10.2026
 */

use async_trait::async_trait;
use relix_driver_api::{
    DatabaseConnection, DocumentConnection, DriverError, KeyValueConnection, QueryExecutor,
    ServerInfo, SqlConnection,
};
use relix_ssh::{TunnelLease, TunnelStatus};
use relix_vpn::VpnLease;
use std::sync::{Mutex, PoisonError};

#[derive(Debug, Default)]
pub struct NetworkLeases {
    pub vpn: Option<VpnLease>,
    pub tunnel: Option<TunnelLease>,
}

impl NetworkLeases {
    pub fn is_empty(&self) -> bool {
        self.vpn.is_none() && self.tunnel.is_none()
    }
}

pub struct LeasedConnection {
    inner: Box<dyn DatabaseConnection>,
    leases: Mutex<NetworkLeases>,
    route: String,
}

impl LeasedConnection {
    pub fn new(inner: Box<dyn DatabaseConnection>, leases: NetworkLeases, route: String) -> Self {
        Self {
            inner,
            leases: Mutex::new(leases),
            route,
        }
    }

    pub fn route(&self) -> &str {
        &self.route
    }

    pub fn tunnel_status(&self) -> Option<TunnelStatus> {
        self.leases
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .tunnel
            .as_ref()
            .map(TunnelLease::status)
    }

    fn release(&self) -> NetworkLeases {
        std::mem::take(&mut *self.leases.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

#[async_trait]
impl DatabaseConnection for LeasedConnection {
    fn server_info(&self) -> ServerInfo {
        self.inner.server_info()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.inner.ping().await
    }

    async fn close(&self) -> Result<(), DriverError> {
        let result = self.inner.close().await;
        drop(self.release());
        result
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        self.inner.as_sql()
    }

    fn as_query(&self) -> Option<&dyn QueryExecutor> {
        self.inner.as_query()
    }

    fn as_key_value(&self) -> Option<&dyn KeyValueConnection> {
        self.inner.as_key_value()
    }

    fn as_document(&self) -> Option<&dyn DocumentConnection> {
        self.inner.as_document()
    }

    fn network_route(&self) -> Option<String> {
        Some(self.route.clone())
    }

    fn network_status(&self) -> Option<String> {
        self.tunnel_status().map(|status| status.label())
    }
}
