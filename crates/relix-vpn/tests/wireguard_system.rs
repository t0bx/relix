/*
 * @author t0bx
 * @since 24.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

use relix_core::{VpnMode, VpnProfile, VpnProvider};
use relix_vpn::{
    Approval, CommandRunner, NetworkInspector, SystemCommand, SystemInspector, SystemRunner,
    VpnError, VpnManager, VpnStatus, probe, split_address,
};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct Environment {
    config: PathBuf,
    interface: String,
    probe: String,
}

fn environment() -> Option<Environment> {
    if !cfg!(target_os = "linux") || std::env::var("RELIX_VPN_INTEGRATION").as_deref() != Ok("1") {
        eprintln!("skipping WireGuard system test: Linux with RELIX_VPN_INTEGRATION=1 only");
        return None;
    }
    let config =
        PathBuf::from(std::env::var("RELIX_TEST_WG_CONFIG").expect("RELIX_TEST_WG_CONFIG"));
    let interface = config.file_stem().unwrap().to_string_lossy().into_owned();
    let probe = std::env::var("RELIX_TEST_WG_PROBE").expect("RELIX_TEST_WG_PROBE");
    Some(Environment {
        config,
        interface,
        probe,
    })
}

fn profile(environment: &Environment, mode: VpnMode) -> VpnProfile {
    let mut profile = VpnProfile::new("Integration WireGuard");
    profile.mode = mode;
    profile.provider = VpnProvider::WireGuard;
    profile.config_path = Some(environment.config.clone());
    profile.probe_address = Some(environment.probe.clone());
    profile.auto_connect = true;
    profile.auto_disconnect = true;
    profile.connect_timeout_secs = 20;
    profile
}

fn interface_up(name: &str) -> bool {
    SystemInspector.interface(name).is_some()
}

async fn echo_through(address: &str) -> Vec<u8> {
    let (host, port) = split_address(address, 7000);
    let mut stream = TcpStream::connect((host.as_str(), port)).await.unwrap();
    stream.write_all(b"through the tunnel").await.unwrap();
    let mut reply = vec![0u8; 18];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut reply))
        .await
        .unwrap()
        .unwrap();
    reply
}

#[tokio::test]
async fn managed_wireguard_profiles_come_up_carry_traffic_and_stop_with_the_last_lease() {
    let Some(environment) = environment() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let manager = VpnManager::system(dir.path());
    let profile = profile(&environment, VpnMode::Managed);
    assert!(!interface_up(&environment.interface));
    assert_eq!(manager.status(&profile), VpnStatus::Disconnected);

    let VpnError::ApprovalRequired { command, .. } = manager
        .acquire(&profile, Approval::NotGranted)
        .await
        .unwrap_err()
    else {
        panic!("starting a tunnel must ask for approval");
    };
    assert!(command.program.ends_with("wg-quick"));
    assert!(
        !interface_up(&environment.interface),
        "nothing may run before approval"
    );

    let first = manager.acquire(&profile, Approval::Granted).await.unwrap();
    let second = manager
        .acquire(&profile, Approval::NotGranted)
        .await
        .unwrap();
    assert!(interface_up(&environment.interface));
    assert!(matches!(
        manager.status(&profile),
        VpnStatus::Connected {
            owned: true,
            references: 2,
            ..
        }
    ));
    let (host, port) = split_address(&environment.probe, 7000);
    assert!(
        probe(&host, port, Duration::from_secs(5))
            .await
            .is_reachable()
    );
    assert_eq!(
        echo_through(&environment.probe).await,
        b"through the tunnel"
    );

    drop(first);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        interface_up(&environment.interface),
        "a lease is still held"
    );
    drop(second);
    for _ in 0..40 {
        if !interface_up(&environment.interface) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(
        !interface_up(&environment.interface),
        "the last lease stops the tunnel"
    );
    assert_eq!(manager.status(&profile), VpnStatus::Disconnected);
}

#[tokio::test]
async fn tunnels_started_outside_relix_are_used_but_never_stopped() {
    let Some(environment) = environment() else {
        return;
    };
    let config = environment.config.display().to_string();
    let up = SystemRunner
        .run(&SystemCommand::new("wg-quick", &["up", &config]))
        .await
        .unwrap();
    assert!(up.success, "{}", up.stderr);
    let dir = tempfile::tempdir().unwrap();
    let manager = VpnManager::system(dir.path());
    for mode in [VpnMode::Existing, VpnMode::Managed] {
        let profile = profile(&environment, mode);
        let lease = manager
            .acquire(&profile, Approval::NotGranted)
            .await
            .unwrap();
        assert!(matches!(
            manager.status(&profile),
            VpnStatus::Connected { owned: false, .. }
        ));
        assert_eq!(
            echo_through(&environment.probe).await,
            b"through the tunnel"
        );
        drop(lease);
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(
            interface_up(&environment.interface),
            "a system tunnel stays up"
        );
        assert!(matches!(
            manager.disconnect(&profile).await,
            Ok(()) | Err(VpnError::NotOwned { .. })
        ));
        assert!(interface_up(&environment.interface));
    }
    let down = SystemRunner
        .run(&SystemCommand::new("wg-quick", &["down", &config]))
        .await
        .unwrap();
    assert!(down.success, "{}", down.stderr);
}
