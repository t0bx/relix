/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::SshError;
use crate::shared_session::SharedSession;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

pub(crate) struct LocalForward {
    local_address: SocketAddr,
    acceptor: JoinHandle<()>,
}

impl LocalForward {
    pub(crate) async fn bind(
        session: Arc<SharedSession>,
        bind_address: &str,
        local_port: Option<u16>,
        target_host: String,
        target_port: u16,
    ) -> Result<Self, SshError> {
        let port = local_port.unwrap_or(0);
        let listener = TcpListener::bind((bind_address, port))
            .await
            .map_err(|error| SshError::PortUnavailable {
                address: bind_address.to_owned(),
                port,
                message: error.to_string(),
            })?;
        let local_address = listener
            .local_addr()
            .map_err(|error| SshError::PortUnavailable {
                address: bind_address.to_owned(),
                port,
                message: error.to_string(),
            })?;
        let acceptor = tokio::spawn(accept_loop(listener, session, target_host, target_port));
        Ok(Self {
            local_address,
            acceptor,
        })
    }

    pub(crate) fn local_address(&self) -> SocketAddr {
        self.local_address
    }

    pub(crate) fn close(&self) {
        self.acceptor.abort();
    }
}

impl Drop for LocalForward {
    fn drop(&mut self) {
        self.acceptor.abort();
    }
}

/// Accepts local connections and pipes each one through its own `direct-tcpip` channel.
/// Connection tasks live in a `JoinSet` owned by this loop, so aborting the loop when
/// the forward is released also tears down every proxied connection.
async fn accept_loop(
    listener: TcpListener,
    session: Arc<SharedSession>,
    target_host: String,
    target_port: u16,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else {
                    continue;
                };
                let session = session.clone();
                let host = target_host.clone();
                connections.spawn(proxy(stream, peer, session, host, target_port));
            }
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

async fn proxy(
    mut stream: TcpStream,
    peer: SocketAddr,
    session: Arc<SharedSession>,
    host: String,
    port: u16,
) {
    let _ = stream.set_nodelay(true);
    let channel = match session.session().await {
        Ok(ssh) => ssh.open_direct(&host, port, peer).await,
        Err(error) => Err(error),
    };
    match channel {
        Ok(mut channel) => {
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut channel).await;
        }
        Err(error) => {
            tracing::warn!(%error, target = %format!("{host}:{port}"), "tunnel connection failed")
        }
    }
}
