/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::SshError;
use crate::hop::{HopSecrets, ResolvedHop};
use crate::known_hosts::HostKeyStore;
use crate::session::SshSession;
use crate::status::TunnelStatus;
use relix_core::ReconnectPolicy;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, watch};
use tokio::task::JoinHandle;

const HEALTH_INTERVAL: Duration = Duration::from_secs(1);

pub(crate) struct SharedSession {
    hops: Vec<ResolvedHop>,
    secrets: Vec<HopSecrets>,
    store: HostKeyStore,
    reconnect: ReconnectPolicy,
    current: Mutex<Option<Arc<SshSession>>>,
    status: watch::Sender<TunnelStatus>,
    monitor: std::sync::Mutex<Option<JoinHandle<()>>>,
}

impl SharedSession {
    pub(crate) fn new(
        hops: Vec<ResolvedHop>,
        secrets: Vec<HopSecrets>,
        store: HostKeyStore,
        reconnect: ReconnectPolicy,
    ) -> Arc<Self> {
        let (status, _) = watch::channel(TunnelStatus::Connecting);
        Arc::new(Self {
            hops,
            secrets,
            store,
            reconnect,
            current: Mutex::new(None),
            status,
            monitor: std::sync::Mutex::new(None),
        })
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<TunnelStatus> {
        self.status.subscribe()
    }

    pub(crate) fn status(&self) -> TunnelStatus {
        self.status.borrow().clone()
    }

    pub(crate) async fn connect_initial(self: &Arc<Self>) -> Result<Arc<SshSession>, SshError> {
        let mut current = self.current.lock().await;
        if let Some(session) = current.as_ref().filter(|session| !session.is_closed()) {
            return Ok(session.clone());
        }
        self.status.send_replace(TunnelStatus::Connecting);
        match SshSession::connect(&self.hops, &self.secrets, &self.store).await {
            Ok(session) => {
                let session = Arc::new(session);
                *current = Some(session.clone());
                self.status.send_replace(TunnelStatus::Connected);
                self.start_monitor();
                Ok(session)
            }
            Err(error) => {
                self.status
                    .send_replace(TunnelStatus::Failed(error.to_string()));
                Err(error)
            }
        }
    }

    /// Returns a live session, re-establishing it when the previous one dropped. Only
    /// transient failures (network, timeouts) are retried, with the configured number of
    /// attempts and delay; authentication or host key problems fail immediately because
    /// retrying them cannot succeed and could lock accounts.
    pub(crate) async fn session(self: &Arc<Self>) -> Result<Arc<SshSession>, SshError> {
        let mut current = self.current.lock().await;
        if let Some(session) = current.as_ref().filter(|session| !session.is_closed()) {
            return Ok(session.clone());
        }
        if current.is_some() && !self.reconnect.enabled {
            self.status
                .send_replace(TunnelStatus::Failed("SSH session lost".into()));
            return Err(SshError::Closed);
        }
        let attempts = self.reconnect.max_attempts.max(1);
        let mut last_error = SshError::Closed;
        for attempt in 1..=attempts {
            self.status
                .send_replace(TunnelStatus::Reconnecting { attempt });
            match SshSession::connect(&self.hops, &self.secrets, &self.store).await {
                Ok(session) => {
                    let session = Arc::new(session);
                    *current = Some(session.clone());
                    self.status.send_replace(TunnelStatus::Connected);
                    return Ok(session);
                }
                Err(error) if error.is_transient() && attempt < attempts => {
                    last_error = error;
                    tokio::time::sleep(Duration::from_secs(self.reconnect.delay_secs)).await;
                }
                Err(error) => {
                    last_error = error;
                    break;
                }
            }
        }
        *current = None;
        self.status
            .send_replace(TunnelStatus::Failed(last_error.to_string()));
        Err(last_error)
    }

    fn start_monitor(self: &Arc<Self>) {
        let mut monitor = self
            .monitor
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if monitor.is_some() {
            return;
        }
        let weak = Arc::downgrade(self);
        *monitor = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(HEALTH_INTERVAL).await;
                let Some(shared) = weak.upgrade() else {
                    return;
                };
                let lost = {
                    let current = shared.current.lock().await;
                    current.as_ref().is_some_and(|session| session.is_closed())
                };
                if lost && shared.status().is_usable() {
                    tracing::warn!(endpoint = %shared.endpoint(), "SSH session lost");
                    let _ = shared.session().await;
                }
            }
        }));
    }

    pub(crate) fn endpoint(&self) -> String {
        self.hops
            .last()
            .map(ResolvedHop::endpoint)
            .unwrap_or_default()
    }

    pub(crate) async fn shutdown(&self) {
        if let Some(monitor) = self
            .monitor
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            monitor.abort();
        }
        if let Some(session) = self.current.lock().await.take() {
            session.disconnect().await;
        }
        self.status.send_replace(TunnelStatus::Closed);
    }
}
