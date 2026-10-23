/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::{HostKeyPrompt, SshError};
use crate::forward::LocalForward;
use crate::hop::{HopSecrets, ResolvedHop};
use crate::known_hosts::HostKeyStore;
use crate::shared_session::SharedSession;
use crate::status::TunnelStatus;
use relix_core::SshServerConfig;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::watch;

#[derive(Debug, Clone)]
pub struct TunnelRequest {
    pub chain: Vec<SshServerConfig>,
    pub secrets: Vec<HopSecrets>,
    pub target_host: String,
    pub target_port: u16,
    pub local_port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ForwardKey {
    session: String,
    target_host: String,
    target_port: u16,
    bind_address: String,
    local_port: Option<u16>,
}

struct SessionEntry {
    shared: Arc<SharedSession>,
    references: usize,
}

struct ForwardEntry {
    forward: Arc<LocalForward>,
    references: usize,
}

#[derive(Default)]
struct State {
    sessions: HashMap<String, SessionEntry>,
    forwards: HashMap<ForwardKey, ForwardEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub endpoint: String,
    pub status: TunnelStatus,
    pub references: usize,
}

#[derive(Clone)]
pub struct TunnelManager {
    state: Arc<Mutex<State>>,
    store: HostKeyStore,
}

impl TunnelManager {
    pub fn new(store: HostKeyStore) -> Self {
        Self {
            state: Arc::default(),
            store,
        }
    }

    /// Opens (or joins) a tunnel to `target` through the configured hop chain. Sessions
    /// are shared by every request with an identical chain configuration and forwards by
    /// every request for the same target; both are reference counted and closed when the
    /// last lease is dropped. A changed configuration produces a new session key, so new
    /// connections use the updated settings while existing ones finish on the old session.
    pub async fn open(&self, request: TunnelRequest) -> Result<TunnelLease, SshError> {
        let hops = request
            .chain
            .iter()
            .map(ResolvedHop::from_config)
            .collect::<Result<Vec<_>, _>>()?;
        let last = request
            .chain
            .last()
            .ok_or_else(|| SshError::Configuration("no SSH server configured".into()))?;
        let session_key = serde_json::to_string(&request.chain)
            .map_err(|error| SshError::Configuration(error.to_string()))?;
        let shared = self.join_session(&session_key, || {
            SharedSession::new(
                hops,
                request.secrets.clone(),
                self.store.clone(),
                last.reconnect,
            )
        });
        if let Err(error) = shared.connect_initial().await {
            self.release(&session_key, None);
            return Err(error);
        }
        let forward_key = ForwardKey {
            session: session_key.clone(),
            target_host: request.target_host.clone(),
            target_port: request.target_port,
            bind_address: last.local_bind_address.clone(),
            local_port: request.local_port,
        };
        let local_address = match self.join_forward(&forward_key) {
            Some(address) => address,
            None => {
                let bound = LocalForward::bind(
                    shared.clone(),
                    &forward_key.bind_address,
                    request.local_port,
                    request.target_host.clone(),
                    request.target_port,
                )
                .await;
                match bound {
                    Ok(forward) => self.insert_forward(&forward_key, forward),
                    Err(error) => {
                        self.release(&session_key, None);
                        return Err(error);
                    }
                }
            }
        };
        Ok(TunnelLease {
            manager: self.clone(),
            session_key,
            forward_key: Some(forward_key),
            local_address,
            status: shared.subscribe(),
            server: last.name.clone(),
        })
    }

    pub fn trust_host_key(&self, prompt: &HostKeyPrompt) -> Result<(), SshError> {
        self.store.trust(prompt)
    }

    pub fn sessions(&self) -> Vec<SessionSummary> {
        let state = self.lock();
        state
            .sessions
            .values()
            .map(|entry| SessionSummary {
                endpoint: entry.shared.endpoint(),
                status: entry.shared.status(),
                references: entry.references,
            })
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn join_session(
        &self,
        key: &str,
        create: impl FnOnce() -> Arc<SharedSession>,
    ) -> Arc<SharedSession> {
        let mut state = self.lock();
        let entry = state
            .sessions
            .entry(key.to_owned())
            .or_insert_with(|| SessionEntry {
                shared: create(),
                references: 0,
            });
        entry.references += 1;
        entry.shared.clone()
    }

    fn join_forward(&self, key: &ForwardKey) -> Option<SocketAddr> {
        let mut state = self.lock();
        let entry = state.forwards.get_mut(key)?;
        entry.references += 1;
        Some(entry.forward.local_address())
    }

    fn insert_forward(&self, key: &ForwardKey, forward: LocalForward) -> SocketAddr {
        let mut state = self.lock();
        let entry = state
            .forwards
            .entry(key.clone())
            .or_insert_with(|| ForwardEntry {
                forward: Arc::new(forward),
                references: 0,
            });
        entry.references += 1;
        entry.forward.local_address()
    }

    fn release(&self, session_key: &str, forward_key: Option<&ForwardKey>) {
        let mut state = self.lock();
        if let Some(key) = forward_key
            && let Some(entry) = state.forwards.get_mut(key)
        {
            entry.references -= 1;
            if entry.references == 0
                && let Some(entry) = state.forwards.remove(key)
            {
                entry.forward.close();
            }
        }
        let Some(entry) = state.sessions.get_mut(session_key) else {
            return;
        };
        entry.references -= 1;
        if entry.references > 0 {
            return;
        }
        if let Some(entry) = state.sessions.remove(session_key) {
            let shared = entry.shared;
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move { shared.shutdown().await });
            }
        }
    }
}

pub struct TunnelLease {
    manager: TunnelManager,
    session_key: String,
    forward_key: Option<ForwardKey>,
    local_address: SocketAddr,
    status: watch::Receiver<TunnelStatus>,
    server: String,
}

impl TunnelLease {
    pub fn local_address(&self) -> SocketAddr {
        self.local_address
    }

    pub fn status(&self) -> TunnelStatus {
        self.status.borrow().clone()
    }

    pub fn server_name(&self) -> &str {
        &self.server
    }
}

impl std::fmt::Debug for TunnelLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TunnelLease")
            .field("server", &self.server)
            .field("local_address", &self.local_address)
            .finish()
    }
}

impl Drop for TunnelLease {
    fn drop(&mut self) {
        self.manager
            .release(&self.session_key, self.forward_key.as_ref());
    }
}
