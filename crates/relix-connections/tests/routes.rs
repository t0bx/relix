/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

use async_trait::async_trait;
use relix_connections::{
    ConnectionError, ConnectionManager, HopSecrets, HostKeyStore, NetworkCredentials, NetworkPlan,
    SshError, TunnelManager, VpnManager,
};
use relix_core::{
    ConnectionConfig, ConnectionSettings, NetworkMode, Secret, SshAuthMethod, VpnProfile,
};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata, ServerInfo,
};
use relix_testing::ssh_server::{PASSWORD, TestServer, ssh_config, start_echo};
use relix_vpn::{NetworkInspector, SystemInspector};
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Default)]
struct EchoDriver {
    seen: Mutex<Vec<ConnectionConfig>>,
}

struct EchoConnection {
    stream: tokio::sync::Mutex<TcpStream>,
}

#[async_trait]
impl DatabaseDriver for EchoDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            "echo",
            "Echo",
            None,
            ConnectionTarget::Network { default_port: 7 },
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities::key_value()
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        self.seen.lock().unwrap().push(config.clone());
        let stream =
            TcpStream::connect((config.host_or_default(), config.settings.port.unwrap_or(7)))
                .await
                .map_err(|error| DriverError::connection(error.to_string()))?;
        let connection = EchoConnection {
            stream: tokio::sync::Mutex::new(stream),
        };
        connection.ping().await?;
        Ok(Box::new(connection))
    }
}

#[async_trait]
impl DatabaseConnection for EchoConnection {
    fn server_info(&self) -> ServerInfo {
        ServerInfo {
            product: "Echo".into(),
            version: "1".into(),
        }
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let mut stream = self.stream.lock().await;
        stream
            .write_all(b"PING")
            .await
            .map_err(|error| DriverError::connection(error.to_string()))?;
        let mut reply = [0u8; 4];
        stream
            .read_exact(&mut reply)
            .await
            .map_err(|error| DriverError::connection(error.to_string()))?;
        if &reply == b"PING" {
            Ok(())
        } else {
            Err(DriverError::internal("unexpected reply"))
        }
    }

    async fn close(&self) -> Result<(), DriverError> {
        let _ = self.stream.lock().await.shutdown().await;
        Ok(())
    }
}

fn config(port: u16) -> ConnectionConfig {
    let settings = ConnectionSettings {
        host: Some("localhost".into()),
        port: Some(port),
        ..ConnectionSettings::default()
    };
    ConnectionConfig::new(settings, Some(Secret::new("db-password")))
}

fn manager(dir: &std::path::Path) -> ConnectionManager {
    ConnectionManager::new(
        TunnelManager::new(HostKeyStore::isolated(dir.join("known_hosts"))),
        VpnManager::system(dir.join("vpn")),
    )
}

fn credentials() -> NetworkCredentials {
    NetworkCredentials {
        ssh: vec![HopSecrets {
            password: Some(Secret::new(PASSWORD)),
            passphrase: None,
        }],
        vpn_approval: None,
    }
}

async fn trusted_ssh_plan(
    manager: &ConnectionManager,
    server: &TestServer,
    driver: &EchoDriver,
    port: u16,
) -> NetworkPlan {
    let plan = NetworkPlan {
        mode: NetworkMode::SshTunnel,
        ssh_chain: vec![ssh_config(
            "Homelab SSH",
            server.port,
            SshAuthMethod::Password,
        )],
        ..NetworkPlan::default()
    };
    match manager
        .connect(driver, &config(port), &plan, &credentials())
        .await
    {
        Err(ConnectionError::Ssh(SshError::HostKeyUnknown(prompt))) => {
            manager.trust_host_key(&prompt).unwrap()
        }
        Err(other) => panic!("unexpected error: {other}"),
        Ok(_) => panic!("an unknown host key must not be accepted silently"),
    }
    plan
}

#[tokio::test]
async fn direct_connections_use_the_configured_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let (echo, _task) = start_echo().await;
    let driver = EchoDriver::default();
    let connection = manager(dir.path())
        .connect(
            &driver,
            &config(echo),
            &NetworkPlan::direct(),
            &NetworkCredentials::default(),
        )
        .await
        .unwrap();
    assert_eq!(connection.network_route(), None);
    let seen = driver.seen.lock().unwrap()[0].clone();
    assert_eq!(seen.settings.port, Some(echo));
    assert_eq!(seen.tls_server_name, None);
}

#[tokio::test]
async fn drivers_connect_through_the_tunnel_endpoint_without_ssh_knowledge() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(None).await;
    let (echo, _task) = start_echo().await;
    let driver = EchoDriver::default();
    let manager = manager(dir.path());
    let plan = trusted_ssh_plan(&manager, &server, &driver, echo).await;
    assert!(
        driver.seen.lock().unwrap().is_empty(),
        "the driver ran before the tunnel was trusted"
    );

    let connection = manager
        .connect(&driver, &config(echo), &plan, &credentials())
        .await
        .unwrap();
    connection.ping().await.unwrap();
    let seen = driver.seen.lock().unwrap()[0].clone();
    assert_eq!(seen.host_or_default(), "127.0.0.1");
    assert_ne!(seen.settings.port, Some(echo));
    assert_eq!(seen.tls_host(), "localhost");
    assert_eq!(
        connection.network_route().as_deref(),
        Some("via SSH Homelab SSH")
    );
    assert_eq!(connection.network_status().as_deref(), Some("connected"));

    connection.close().await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(manager.ssh_sessions().is_empty());
}

#[tokio::test]
async fn closing_one_connection_keeps_the_shared_session_for_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(None).await;
    let (echo, _task) = start_echo().await;
    let driver = EchoDriver::default();
    let manager = manager(dir.path());
    let plan = trusted_ssh_plan(&manager, &server, &driver, echo).await;
    let first = manager
        .connect(&driver, &config(echo), &plan, &credentials())
        .await
        .unwrap();
    let second = manager
        .connect(&driver, &config(echo), &plan, &credentials())
        .await
        .unwrap();
    assert_eq!(manager.ssh_sessions().len(), 1);
    assert_eq!(manager.ssh_sessions()[0].references, 2);
    first.close().await.unwrap();
    second.ping().await.unwrap();
    assert_eq!(manager.ssh_sessions()[0].references, 1);
    second.close().await.unwrap();
}

#[tokio::test]
async fn existing_vpn_and_ssh_tunnel_combine() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(None).await;
    let (echo, _task) = start_echo().await;
    let driver = EchoDriver::default();
    let manager = manager(dir.path());
    let loopback = SystemInspector
        .interfaces()
        .into_iter()
        .find(|interface| {
            interface
                .addresses
                .iter()
                .any(std::net::IpAddr::is_loopback)
        })
        .expect("a loopback interface");
    let mut vpn = VpnProfile::new("Corporate WireGuard");
    vpn.interface = Some(loopback.name);
    vpn.probe_address = Some(format!("127.0.0.1:{}", server.port));
    let mut plan = trusted_ssh_plan(&manager, &server, &driver, echo).await;
    plan.mode = NetworkMode::VpnSshTunnel;
    plan.vpn = Some(vpn.clone());

    let connection = manager
        .connect(&driver, &config(echo), &plan, &credentials())
        .await
        .unwrap();
    connection.ping().await.unwrap();
    assert_eq!(
        connection.network_route().as_deref(),
        Some("via VPN Corporate WireGuard + SSH Homelab SSH")
    );
    connection.close().await.unwrap();
    assert!(matches!(
        manager.vpn_status(&vpn),
        relix_vpn::VpnStatus::Connected { owned: false, .. }
    ));

    let mut down = vpn.clone();
    down.interface = Some("relix-missing0".into());
    plan.vpn = Some(down);
    let error = manager
        .connect(&driver, &config(echo), &plan, &credentials())
        .await
        .err()
        .expect("a missing VPN interface must fail");
    assert_eq!(error.stage(), "VPN");
}

#[tokio::test]
async fn ssh_authentication_failures_stop_before_the_driver() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(None).await;
    let (echo, _task) = start_echo().await;
    let driver = EchoDriver::default();
    let manager = manager(dir.path());
    let plan = trusted_ssh_plan(&manager, &server, &driver, echo).await;
    let wrong = NetworkCredentials {
        ssh: vec![HopSecrets {
            password: Some(Secret::new("nope")),
            passphrase: None,
        }],
        vpn_approval: None,
    };
    let error = manager
        .connect(&driver, &config(echo), &plan, &wrong)
        .await
        .err()
        .expect("wrong SSH password");
    assert!(matches!(
        error,
        ConnectionError::Ssh(SshError::Authentication { .. })
    ));
    assert!(driver.seen.lock().unwrap().is_empty());
}
