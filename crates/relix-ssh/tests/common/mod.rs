/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code)]

use relix_core::{HostKeyPolicy, SshAuthMethod, SshServerConfig};
use relix_ssh::{HostKeyStore, SshError, TunnelManager};
use russh::Channel;
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey, PublicKey};
use russh::server::{Auth, ChannelOpenHandle, Msg, Session};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

pub const USER: &str = "relix";
pub const PASSWORD: &str = "correct horse battery staple";

pub struct TestServer {
    pub port: u16,
    pub host_key: PrivateKey,
    accept: Option<JoinHandle<()>>,
    sessions: Arc<Mutex<Vec<russh::server::Handle>>>,
}

impl TestServer {
    pub async fn start(authorized: Option<PublicKey>) -> Self {
        Self::start_on(0, generate_key(), authorized).await
    }

    pub async fn start_on(port: u16, host_key: PrivateKey, authorized: Option<PublicKey>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let config = Arc::new(russh::server::Config {
            keys: vec![host_key.clone()],
            auth_rejection_time: Duration::from_millis(5),
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..russh::server::Config::default()
        });
        let sessions: Arc<Mutex<Vec<russh::server::Handle>>> = Arc::default();
        let tracked = sessions.clone();
        let accept = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    return;
                };
                let handler = ServerSession {
                    authorized: authorized.clone(),
                };
                let config = config.clone();
                if let Ok(running) = russh::server::run_stream(config, socket, handler).await {
                    tracked.lock().unwrap().push(running.handle());
                    tokio::spawn(async move {
                        let _ = running.await;
                    });
                }
            }
        });
        Self {
            port,
            host_key,
            accept: Some(accept),
            sessions,
        }
    }

    pub async fn shutdown(mut self) {
        if let Some(accept) = self.accept.take() {
            accept.abort();
            let _ = accept.await;
        }
        let sessions: Vec<_> = self.sessions.lock().unwrap().drain(..).collect();
        for session in sessions {
            let _ = session
                .disconnect(
                    russh::Disconnect::ByApplication,
                    "shutdown".into(),
                    "en".into(),
                )
                .await;
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(accept) = self.accept.take() {
            accept.abort();
        }
    }
}

struct ServerSession {
    authorized: Option<PublicKey>,
}

impl russh::server::Handler for ServerSession {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == USER && password == PASSWORD {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn auth_publickey(&mut self, user: &str, key: &PublicKey) -> Result<Auth, Self::Error> {
        let known = self
            .authorized
            .as_ref()
            .is_some_and(|authorized| authorized.key_data() == key.key_data());
        Ok(if user == USER && known {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let target = format!("{host_to_connect}:{port_to_connect}");
        match TcpStream::connect(target).await {
            Ok(mut upstream) => {
                reply.accept().await;
                tokio::spawn(async move {
                    let mut stream = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
                });
            }
            Err(_) => {
                reply.reject(russh::ChannelOpenFailure::ConnectFailed).await;
            }
        }
        Ok(())
    }
}

pub fn generate_key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
}

pub fn write_key(dir: &Path, key: &PrivateKey, passphrase: Option<&str>) -> PathBuf {
    let key = match passphrase {
        Some(passphrase) => key.encrypt(&mut rand::rng(), passphrase).unwrap(),
        None => key.clone(),
    };
    let path = dir.join(format!("id_{}", rand::random::<u32>()));
    std::fs::write(&path, key.to_openssh(LineEnding::LF).unwrap().as_bytes()).unwrap();
    path
}

pub fn ssh_config(name: &str, port: u16, auth: SshAuthMethod) -> SshServerConfig {
    let mut config = SshServerConfig::new(name, "127.0.0.1");
    config.port = port;
    config.username = USER.into();
    config.auth = auth;
    config.host_key_policy = HostKeyPolicy::AskOnFirstUse;
    config.connect_timeout_secs = 5;
    config.keepalive_interval_secs = 0;
    config.reconnect.delay_secs = 0;
    config
}

pub fn manager(dir: &Path) -> TunnelManager {
    TunnelManager::new(HostKeyStore::isolated(dir.join("known_hosts")))
}

pub fn trust_all(manager: &TunnelManager, error: SshError) {
    match error {
        SshError::HostKeyUnknown(prompt) => manager.trust_host_key(&prompt).unwrap(),
        other => panic!("expected an unknown host key, got {other}"),
    }
}

pub async fn start_echo() -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buffer = [0u8; 1024];
                loop {
                    match socket.read(&mut buffer).await {
                        Ok(0) | Err(_) => return,
                        Ok(read) => {
                            if socket.write_all(&buffer[..read]).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            });
        }
    });
    (port, task)
}

pub async fn echo_through(port: u16, message: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(message).await.unwrap();
    let mut received = vec![0u8; message.len()];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut received))
        .await
        .expect("echo timed out")
        .unwrap();
    received
}
