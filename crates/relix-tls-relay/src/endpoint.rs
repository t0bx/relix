/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::relay::{RelayGuard, start_relay};
use crate::target::TlsTarget;
use relix_core::{ConnectionConfig, TlsMode};
use relix_driver_api::{DriverError, with_timeout};
use std::sync::Arc;

#[derive(Clone)]
pub struct DriverEndpoint {
    host: String,
    port: u16,
    relay: Option<Arc<RelayGuard>>,
}

impl DriverEndpoint {
    fn direct(host: &str, port: u16) -> Self {
        Self {
            host: host.to_owned(),
            port,
            relay: None,
        }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn address(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    pub fn is_encrypted(&self) -> bool {
        self.relay.is_some()
    }
}

/// Decides where a driver that speaks plain TCP should connect. Without TLS that is the
/// configured server. Otherwise one TLS handshake is made up front (bounded by the
/// connect timeout) so misconfigurations surface as `ErrorKind::Tls` at connect time;
/// on success a loopback relay is started and the driver connects to it in plain text.
/// `Prefer` falls back to the plain server when the handshake fails, because these
/// protocols cannot negotiate TLS on an established connection. Certificates are checked
/// against `ConnectionConfig::tls_host`, so verification keeps working when the TCP
/// connection is redirected through an SSH tunnel.
pub async fn open_endpoint(
    config: &ConnectionConfig,
    default_port: u16,
) -> Result<DriverEndpoint, DriverError> {
    let host = config.host_or_default();
    let port = config.settings.port.unwrap_or(default_port);
    relix_tls::install_default_provider();
    let Some(tls) = relix_tls::client_config(&config.settings.tls).await? else {
        return Ok(DriverEndpoint::direct(host, port));
    };
    let target = TlsTarget::new(host, port, config.tls_host(), tls)?;
    let probe = with_timeout(config.settings.connect_timeout(), target.connect()).await;
    match probe {
        Ok(_) => {}
        Err(_) if config.settings.tls.mode == TlsMode::Prefer => {
            return Ok(DriverEndpoint::direct(host, port));
        }
        Err(error) => return Err(error),
    }
    let (address, guard) = start_relay(target).await.map_err(|error| {
        DriverError::internal(format!("cannot start the local TLS relay: {error}"))
    })?;
    Ok(DriverEndpoint {
        host: address.ip().to_string(),
        port: address.port(),
        relay: Some(Arc::new(guard)),
    })
}
