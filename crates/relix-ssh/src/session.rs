/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::auth::authenticate;
use crate::error::SshError;
use crate::handler::{ClientHandler, take_rejection};
use crate::hop::{HopSecrets, ResolvedHop};
use crate::known_hosts::HostKeyStore;
use russh::client::{self, Handle, Msg};
use russh::{ChannelStream, Disconnect};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

const KEEPALIVE_MAX_MISSED: usize = 3;

pub struct SshSession {
    handles: Vec<Handle<ClientHandler>>,
    target: String,
}

impl SshSession {
    /// Connects through every hop in order: the first hop over TCP, each further hop
    /// through a `direct-tcpip` channel of the previous one, so only the first server
    /// has to be reachable from this machine. Each hop verifies its own host key and
    /// authenticates with its own credentials.
    pub async fn connect(
        hops: &[ResolvedHop],
        secrets: &[HopSecrets],
        store: &HostKeyStore,
    ) -> Result<Self, SshError> {
        let Some(first) = hops.first() else {
            return Err(SshError::Configuration("no SSH server configured".into()));
        };
        let mut handles: Vec<Handle<ClientHandler>> = Vec::with_capacity(hops.len());
        for (index, hop) in hops.iter().enumerate() {
            let hop_secrets = secrets.get(index).cloned().unwrap_or_default();
            let handle = match handles.last() {
                None => {
                    let stream = tcp_connect(hop).await?;
                    connect_hop(hop, &hop_secrets, store, stream).await?
                }
                Some(previous) => {
                    let channel = previous
                        .channel_open_direct_tcpip(&hop.host, u32::from(hop.port), "127.0.0.1", 0)
                        .await
                        .map_err(|error| {
                            SshError::connection(
                                &hop.endpoint(),
                                format!("jump host refused the connection: {error}"),
                            )
                        })?;
                    connect_hop(hop, &hop_secrets, store, channel.into_stream()).await?
                }
            };
            handles.push(handle);
        }
        Ok(Self {
            target: hops.last().unwrap_or(first).endpoint(),
            handles,
        })
    }

    pub fn is_closed(&self) -> bool {
        self.handles.iter().any(Handle::is_closed)
    }

    pub fn endpoint(&self) -> &str {
        &self.target
    }

    pub async fn open_direct(
        &self,
        host: &str,
        port: u16,
        originator: SocketAddr,
    ) -> Result<ChannelStream<Msg>, SshError> {
        let handle = self.handles.last().ok_or(SshError::Closed)?;
        let channel = handle
            .channel_open_direct_tcpip(
                host,
                u32::from(port),
                originator.ip().to_string(),
                u32::from(originator.port()),
            )
            .await
            .map_err(|error| SshError::Forwarding {
                target: format!("{host}:{port}"),
                message: error.to_string(),
            })?;
        Ok(channel.into_stream())
    }

    pub async fn disconnect(&self) {
        for handle in self.handles.iter().rev() {
            let _ = handle
                .disconnect(Disconnect::ByApplication, "closed by relix", "en")
                .await;
        }
    }
}

async fn tcp_connect(hop: &ResolvedHop) -> Result<TcpStream, SshError> {
    let endpoint = hop.endpoint();
    let connect = TcpStream::connect((hop.host.as_str(), hop.port));
    let stream = tokio::time::timeout(hop.connect_timeout, connect)
        .await
        .map_err(|_| timeout(hop))?
        .map_err(|error| SshError::connection(&endpoint, error))?;
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

async fn connect_hop<S>(
    hop: &ResolvedHop,
    secrets: &HopSecrets,
    store: &HostKeyStore,
    stream: S,
) -> Result<Handle<ClientHandler>, SshError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let config = Arc::new(client::Config {
        keepalive_interval: hop.keepalive,
        keepalive_max: KEEPALIVE_MAX_MISSED,
        ..client::Config::default()
    });
    let (handler, rejection) = ClientHandler::new(store.clone(), hop.clone());
    let establish = async {
        let mut handle = client::connect_stream(config, stream, handler)
            .await
            .map_err(|error| {
                take_rejection(&rejection)
                    .unwrap_or_else(|| SshError::connection(&hop.endpoint(), error))
            })?;
        authenticate(&mut handle, hop, secrets).await?;
        Ok(handle)
    };
    tokio::time::timeout(hop.connect_timeout, establish)
        .await
        .map_err(|_| timeout(hop))?
}

fn timeout(hop: &ResolvedHop) -> SshError {
    SshError::Timeout {
        endpoint: hop.endpoint(),
        seconds: hop.connect_timeout.as_secs(),
    }
}
