/*
 * @author t0bx
 * @since 23.10.2026
 */

mod command;
mod elevation;
mod error;
mod manager;
mod network;
pub mod profile_file;
mod provider;
mod tools;

pub use command::{CommandOutput, CommandRunner, SystemCommand, SystemRunner};
pub use elevation::ElevationMethod;
pub use error::VpnError;
pub use manager::{Approval, PathLocator, ToolLocator, VpnLease, VpnManager, VpnStatus};
pub use network::{
    InterfaceInfo, NetworkInspector, ProbeOutcome, SystemInspector, probe, split_address,
};
pub use tools::find_executable;
