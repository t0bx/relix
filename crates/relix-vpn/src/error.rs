/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::command::SystemCommand;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VpnError {
    #[error("invalid VPN profile '{profile}': {message}")]
    Configuration { profile: String, message: String },
    #[error("cannot read VPN configuration {}: {message}", path.display())]
    ConfigFile { path: PathBuf, message: String },
    #[error("'{tool}' was not found; install it to manage {provider} profiles")]
    ToolMissing { tool: String, provider: String },
    #[error("managed VPN profiles are not supported on this platform")]
    UnsupportedPlatform,
    #[error("connecting '{profile}' needs administrator rights; approve the command to continue")]
    ApprovalRequired {
        profile: String,
        command: SystemCommand,
    },
    #[error("'{profile}' is not connected and is not set to connect automatically")]
    NotConnected { profile: String },
    #[error("administrator rights were not granted for '{profile}'")]
    PrivilegeDenied { profile: String },
    #[error("'{profile}' failed to start: {message}")]
    StartFailed { profile: String, message: String },
    #[error("'{profile}' did not come up within {seconds}s")]
    Timeout { profile: String, seconds: u64 },
    #[error("'{profile}' routes {routes} already used by the active VPN '{other}'")]
    RouteConflict {
        profile: String,
        other: String,
        routes: String,
    },
    #[error("network interface '{interface}' of '{profile}' is not up")]
    InterfaceDown { profile: String, interface: String },
    #[error("{target} is not reachable through '{profile}': {message}")]
    Unreachable {
        profile: String,
        target: String,
        message: String,
    },
    #[error("'{profile}' is still used by {count} connection(s)")]
    InUse { profile: String, count: usize },
    #[error("'{profile}' is managed outside Relix and is never disconnected by it")]
    NotOwned { profile: String },
}
