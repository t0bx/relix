/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code, clippy::unwrap_used)]

use relix_ssh::{HostKeyStore, SshError, TunnelManager};
pub use relix_testing::ssh_server::*;
use std::path::Path;

pub fn manager(dir: &Path) -> TunnelManager {
    TunnelManager::new(HostKeyStore::isolated(dir.join("known_hosts")))
}

pub fn trust_all(manager: &TunnelManager, error: SshError) {
    match error {
        SshError::HostKeyUnknown(prompt) => manager.trust_host_key(&prompt).unwrap(),
        other => panic!("expected an unknown host key, got {other}"),
    }
}
