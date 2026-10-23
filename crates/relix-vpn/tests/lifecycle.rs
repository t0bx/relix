/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

use async_trait::async_trait;
use relix_core::{VpnMode, VpnProfile, VpnProvider};
use relix_vpn::{
    Approval, CommandOutput, CommandRunner, ElevationMethod, InterfaceInfo, NetworkInspector,
    SystemCommand, ToolLocator, VpnError, VpnManager, VpnStatus,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct SimulatedHost {
    interfaces: Mutex<Vec<String>>,
    commands: Mutex<Vec<SystemCommand>>,
    deny: Mutex<bool>,
    fail: Mutex<bool>,
}

impl SimulatedHost {
    fn bring_up(&self, name: &str) {
        self.interfaces.lock().unwrap().push(name.to_owned());
    }

    fn privileged_commands(&self) -> Vec<String> {
        self.commands
            .lock()
            .unwrap()
            .iter()
            .map(|command| command.args.join(" "))
            .collect()
    }
}

struct Inspector(Arc<SimulatedHost>);

impl NetworkInspector for Inspector {
    fn interfaces(&self) -> Vec<InterfaceInfo> {
        self.0
            .interfaces
            .lock()
            .unwrap()
            .iter()
            .map(|name| InterfaceInfo {
                name: name.clone(),
                addresses: vec!["10.10.0.2".parse().unwrap()],
            })
            .collect()
    }

    fn read_file(&self, _path: &Path) -> Option<String> {
        None
    }
}

struct Runner(Arc<SimulatedHost>);

#[async_trait]
impl CommandRunner for Runner {
    async fn run(&self, command: &SystemCommand) -> std::io::Result<CommandOutput> {
        self.0.commands.lock().unwrap().push(command.clone());
        if *self.0.deny.lock().unwrap() {
            return Ok(CommandOutput {
                success: false,
                code: Some(126),
                ..CommandOutput::default()
            });
        }
        if *self.0.fail.lock().unwrap() {
            return Ok(CommandOutput {
                success: false,
                code: Some(1),
                stderr: "Line unrecognized: `Foo=bar'\n".into(),
                ..CommandOutput::default()
            });
        }
        let args = command.args.join(" ");
        let stem = command
            .args
            .last()
            .and_then(|path| Path::new(path).file_stem())
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        if args.contains(" up ") || args.contains("/installtunnelservice") {
            self.0.bring_up(&stem);
        } else if args.contains(" down ") || args.contains("/uninstalltunnelservice") {
            self.0
                .interfaces
                .lock()
                .unwrap()
                .retain(|name| *name != stem);
        }
        Ok(CommandOutput {
            success: true,
            code: Some(0),
            ..CommandOutput::default()
        })
    }
}

struct Tools;

impl ToolLocator for Tools {
    fn locate(&self, tool: &str) -> Option<PathBuf> {
        Some(PathBuf::from(format!("/usr/bin/{tool}")))
    }
}

struct NoTools;

impl ToolLocator for NoTools {
    fn locate(&self, _tool: &str) -> Option<PathBuf> {
        None
    }
}

fn manager(host: &Arc<SimulatedHost>, dir: &Path) -> VpnManager {
    VpnManager::new(
        Arc::new(Runner(host.clone())),
        Arc::new(Inspector(host.clone())),
        Arc::new(Tools),
        Some(ElevationMethod::Pkexec),
        dir.join("vpn"),
    )
}

fn wireguard_profile(dir: &Path, name: &str, file: &str, routes: &str) -> VpnProfile {
    let path = dir.join(format!("{file}.conf"));
    std::fs::write(
        &path,
        format!(
            "[Interface]\nPrivateKey = aGVsbG8=\n[Peer]\nPublicKey = d29ybGQ=\nAllowedIPs = {routes}\n"
        ),
    )
    .unwrap();
    let mut profile = VpnProfile::new(name);
    profile.mode = VpnMode::Managed;
    profile.provider = VpnProvider::WireGuard;
    profile.config_path = Some(path);
    profile.interface = Some(file.to_owned());
    profile.connect_timeout_secs = 1;
    profile
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(50)).await;
}

#[tokio::test]
async fn existing_vpns_are_verified_and_never_touched() {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(SimulatedHost::default());
    let manager = manager(&host, dir.path());
    let mut profile = VpnProfile::new("Company VPN");
    profile.interface = Some("utun4".into());

    let missing = manager
        .acquire(&profile, Approval::Granted)
        .await
        .unwrap_err();
    assert!(matches!(missing, VpnError::InterfaceDown { .. }));

    host.bring_up("utun4");
    let lease = manager
        .acquire(&profile, Approval::NotGranted)
        .await
        .unwrap();
    assert!(matches!(
        manager.status(&profile),
        VpnStatus::Connected { owned: false, .. }
    ));
    assert!(matches!(
        manager.disconnect(&profile).await,
        Err(VpnError::NotOwned { .. })
    ));
    drop(lease);
    settle().await;
    assert!(host.privileged_commands().is_empty());
}

#[tokio::test]
async fn managed_profiles_need_approval_and_stop_with_their_last_connection() {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(SimulatedHost::default());
    let manager = manager(&host, dir.path());
    let mut profile = wireguard_profile(dir.path(), "Production WireGuard", "prod", "10.10.0.0/24");

    let not_connected = manager
        .acquire(&profile, Approval::NotGranted)
        .await
        .unwrap_err();
    assert!(matches!(not_connected, VpnError::NotConnected { .. }));
    profile.auto_connect = true;
    let VpnError::ApprovalRequired { command, .. } = manager
        .acquire(&profile, Approval::NotGranted)
        .await
        .unwrap_err()
    else {
        panic!("expected an approval request");
    };
    assert!(command.program.starts_with("/usr/bin/"));
    assert!(host.privileged_commands().is_empty());

    let postgres = manager.acquire(&profile, Approval::Granted).await.unwrap();
    let redis = manager
        .acquire(&profile, Approval::NotGranted)
        .await
        .unwrap();
    assert_eq!(host.privileged_commands().len(), 1);
    assert!(host.privileged_commands()[0].starts_with("/usr/bin/"));
    assert!(matches!(
        manager.status(&profile),
        VpnStatus::Connected {
            owned: true,
            references: 2,
            ..
        }
    ));
    assert!(matches!(
        manager.disconnect(&profile).await,
        Err(VpnError::InUse { count: 2, .. })
    ));

    drop(postgres);
    settle().await;
    assert_eq!(host.privileged_commands().len(), 1);
    drop(redis);
    settle().await;
    let commands = host.privileged_commands();
    assert_eq!(commands.len(), 2);
    assert!(commands[1].contains("down") || commands[1].contains("uninstall"));
    assert_eq!(manager.status(&profile), VpnStatus::Disconnected);
}

#[tokio::test]
async fn profiles_without_auto_disconnect_stay_up() {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(SimulatedHost::default());
    let manager = manager(&host, dir.path());
    let mut profile = wireguard_profile(dir.path(), "Office", "office", "192.168.50.0/24");
    profile.auto_connect = true;
    profile.auto_disconnect = false;
    drop(manager.acquire(&profile, Approval::Granted).await.unwrap());
    settle().await;
    assert_eq!(host.privileged_commands().len(), 1);
    manager.disconnect(&profile).await.unwrap();
    assert_eq!(host.privileged_commands().len(), 2);
}

#[tokio::test]
async fn shutdown_stops_owned_auto_disconnect_tunnels_only() {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(SimulatedHost::default());
    let manager = manager(&host, dir.path());
    let office = wireguard_profile(dir.path(), "Office", "office", "10.10.0.0/16");
    let mut lab = wireguard_profile(dir.path(), "Lab", "lab", "172.16.0.0/16");
    lab.auto_disconnect = false;
    manager.connect(&office, Approval::Granted).await.unwrap();
    manager.connect(&lab, Approval::Granted).await.unwrap();
    manager.shutdown().await;
    let commands = host.privileged_commands();
    assert_eq!(commands.len(), 3);
    assert!(commands[2].contains("office"));
    assert_eq!(manager.status(&office), VpnStatus::Disconnected);
    assert!(matches!(
        manager.status(&lab),
        VpnStatus::Connected { owned: true, .. }
    ));
}

#[tokio::test]
async fn already_running_managed_profiles_are_not_owned() {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(SimulatedHost::default());
    let manager = manager(&host, dir.path());
    let mut profile = wireguard_profile(dir.path(), "Dev VPN", "dev", "10.30.0.0/16");
    profile.auto_connect = true;
    host.bring_up("dev");
    drop(
        manager
            .acquire(&profile, Approval::NotGranted)
            .await
            .unwrap(),
    );
    settle().await;
    assert!(host.privileged_commands().is_empty());
}

#[tokio::test]
async fn privilege_denial_and_start_failures_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(SimulatedHost::default());
    let manager = manager(&host, dir.path());
    let profile = wireguard_profile(dir.path(), "Prod", "prod", "10.10.0.0/24");

    *host.deny.lock().unwrap() = true;
    let denied = manager
        .connect(&profile, Approval::Granted)
        .await
        .unwrap_err();
    assert!(matches!(denied, VpnError::PrivilegeDenied { .. }));

    *host.deny.lock().unwrap() = false;
    *host.fail.lock().unwrap() = true;
    let failed = manager
        .connect(&profile, Approval::Granted)
        .await
        .unwrap_err();
    assert!(failed.to_string().contains("Line unrecognized"));
    assert!(matches!(manager.status(&profile), VpnStatus::Failed(_)));
}

#[tokio::test]
async fn overlapping_routes_are_refused_instead_of_disrupting_a_tunnel() {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(SimulatedHost::default());
    let manager = manager(&host, dir.path());
    let office = wireguard_profile(dir.path(), "Office", "office", "10.10.0.0/16");
    let datacenter = wireguard_profile(dir.path(), "Datacenter", "dc", "10.10.5.0/24");
    manager.connect(&office, Approval::Granted).await.unwrap();
    let conflict = manager
        .connect(&datacenter, Approval::Granted)
        .await
        .unwrap_err();
    let VpnError::RouteConflict { other, routes, .. } = conflict else {
        panic!("expected a route conflict");
    };
    assert_eq!(other, "Office");
    assert_eq!(routes, "10.10.5.0/24");
    assert_eq!(host.privileged_commands().len(), 1);
}

#[tokio::test]
async fn missing_tools_and_timeouts_fail_clearly() {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(SimulatedHost::default());
    let without_tools = VpnManager::new(
        Arc::new(Runner(host.clone())),
        Arc::new(Inspector(host.clone())),
        Arc::new(NoTools),
        Some(ElevationMethod::Pkexec),
        dir.path(),
    );
    let profile = wireguard_profile(dir.path(), "Prod", "prod", "10.10.0.0/24");
    let missing = without_tools
        .connect(&profile, Approval::Granted)
        .await
        .unwrap_err();
    assert!(matches!(missing, VpnError::ToolMissing { .. }));

    let manager = manager(&host, dir.path());
    let mut never_up = profile.clone();
    never_up.interface = Some("wg-never".into());
    let timeout = manager
        .connect(&never_up, Approval::Granted)
        .await
        .unwrap_err();
    assert!(matches!(timeout, VpnError::Timeout { .. }));
}
