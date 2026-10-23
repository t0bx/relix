/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::target::TlsTarget;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(50);

pub(crate) struct RelayGuard {
    task: JoinHandle<()>,
}

impl Drop for RelayGuard {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Listens on an ephemeral loopback port and forwards every accepted connection over TLS
/// to the target. Dropping the guard stops the listener and, because the forwarding
/// tasks live in a `JoinSet` owned by the accept loop, every open forwarded connection.
pub(crate) async fn start_relay(target: TlsTarget) -> io::Result<(SocketAddr, RelayGuard)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(accept_loop(listener, target));
    Ok((address, RelayGuard { task }))
}

async fn accept_loop(listener: TcpListener, target: TlsTarget) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((client, _)) => {
                    connections.spawn(forward(client, target.clone()));
                }
                Err(_) => tokio::time::sleep(ACCEPT_RETRY_DELAY).await,
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

async fn forward(mut client: TcpStream, target: TlsTarget) {
    let _ = client.set_nodelay(true);
    let Ok(mut upstream) = target.connect().await else {
        return;
    };
    let _ = copy_bidirectional(&mut client, &mut upstream).await;
}
