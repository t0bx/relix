/*
 * @author t0bx
 * @since 23.10.2026
 */

mod error;
mod leased;
mod manager;
mod plan;

pub use error::ConnectionError;
pub use leased::{LeasedConnection, NetworkLeases};
pub use manager::{ConnectionManager, NetworkCredentials};
pub use plan::{NetworkPlan, plan_network};
pub use relix_ssh::{
    CredentialKind, HopSecrets, HostKeyPrompt, HostKeyStore, SshError, TunnelManager, TunnelStatus,
};
pub use relix_vpn::{Approval, VpnError, VpnManager, VpnStatus};
