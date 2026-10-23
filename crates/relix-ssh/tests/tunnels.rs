/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use relix_core::{Secret, SshAuthMethod};
use relix_ssh::{CredentialKind, HopSecrets, SshError, TunnelRequest, TunnelStatus};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};

fn password_secrets() -> HopSecrets {
    HopSecrets {
        password: Some(Secret::new(PASSWORD)),
        passphrase: None,
    }
}

fn request(
    chain: Vec<relix_core::SshServerConfig>,
    secrets: Vec<HopSecrets>,
    target: u16,
) -> TunnelRequest {
    TunnelRequest {
        chain,
        secrets,
        target_host: "127.0.0.1".into(),
        target_port: target,
        local_port: None,
    }
}

#[tokio::test]
async fn password_tunnel_forwards_traffic_after_trusting_the_host_key() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(None).await;
    let (echo, _echo_task) = start_echo().await;
    let manager = manager(dir.path());
    let config = ssh_config("Bastion", server.port, SshAuthMethod::Password);
    let first = manager
        .open(request(
            vec![config.clone()],
            vec![password_secrets()],
            echo,
        ))
        .await
        .unwrap_err();
    trust_all(&manager, first);
    let lease = manager
        .open(request(vec![config], vec![password_secrets()], echo))
        .await
        .unwrap();
    assert_eq!(lease.status(), TunnelStatus::Connected);
    assert!(lease.local_address().ip().is_loopback());
    assert_eq!(
        echo_through(lease.local_address().port(), b"select 1").await,
        b"select 1"
    );
}

#[tokio::test]
async fn invalid_and_missing_credentials_are_distinguished() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(None).await;
    let manager = manager(dir.path());
    let config = ssh_config("Bastion", server.port, SshAuthMethod::Password);
    let unknown = manager
        .open(request(vec![config.clone()], vec![password_secrets()], 1))
        .await
        .unwrap_err();
    trust_all(&manager, unknown);
    let wrong = HopSecrets {
        password: Some(Secret::new("wrong")),
        passphrase: None,
    };
    let error = manager
        .open(request(vec![config.clone()], vec![wrong], 1))
        .await
        .unwrap_err();
    assert!(matches!(error, SshError::Authentication { .. }), "{error}");
    assert!(!error.to_string().contains("wrong"));
    let missing = manager
        .open(request(vec![config], vec![HopSecrets::default()], 1))
        .await
        .unwrap_err();
    assert!(matches!(
        missing,
        SshError::CredentialsRequired {
            kind: CredentialKind::Password,
            ..
        }
    ));
    assert!(manager.sessions().is_empty());
}

#[tokio::test]
async fn private_keys_with_and_without_passphrase_authenticate() {
    let dir = tempfile::tempdir().unwrap();
    let client_key = generate_key();
    let server = TestServer::start(Some(client_key.public_key().clone())).await;
    let (echo, _echo_task) = start_echo().await;
    let manager = manager(dir.path());

    let mut plain = ssh_config("Plain key", server.port, SshAuthMethod::PrivateKey);
    plain.private_key = Some(write_key(dir.path(), &client_key, None));
    let unknown = manager
        .open(request(
            vec![plain.clone()],
            vec![HopSecrets::default()],
            echo,
        ))
        .await
        .unwrap_err();
    trust_all(&manager, unknown);
    let lease = manager
        .open(request(vec![plain], vec![HopSecrets::default()], echo))
        .await
        .unwrap();
    assert_eq!(
        echo_through(lease.local_address().port(), b"key").await,
        b"key"
    );

    let mut encrypted = ssh_config("Encrypted key", server.port, SshAuthMethod::PrivateKey);
    encrypted.private_key = Some(write_key(dir.path(), &client_key, Some("s3cret")));
    let missing = manager
        .open(request(
            vec![encrypted.clone()],
            vec![HopSecrets::default()],
            echo,
        ))
        .await
        .unwrap_err();
    assert!(matches!(
        missing,
        SshError::CredentialsRequired {
            kind: CredentialKind::Passphrase,
            ..
        }
    ));
    let with_passphrase = HopSecrets {
        password: None,
        passphrase: Some(Secret::new("s3cret")),
    };
    let lease = manager
        .open(request(vec![encrypted], vec![with_passphrase], echo))
        .await
        .unwrap();
    assert_eq!(
        echo_through(lease.local_address().port(), b"enc").await,
        b"enc"
    );

    let mut foreign = ssh_config("Foreign key", server.port, SshAuthMethod::PrivateKey);
    foreign.private_key = Some(write_key(dir.path(), &generate_key(), None));
    let rejected = manager
        .open(request(vec![foreign], vec![HopSecrets::default()], echo))
        .await
        .unwrap_err();
    assert!(matches!(rejected, SshError::Authentication { .. }));
}

#[tokio::test]
async fn a_changed_host_key_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(None).await;
    let port = server.port;
    let manager = manager(dir.path());
    let config = ssh_config("Bastion", port, SshAuthMethod::Password);
    let unknown = manager
        .open(request(vec![config.clone()], vec![password_secrets()], 1))
        .await
        .unwrap_err();
    trust_all(&manager, unknown);
    server.shutdown().await;
    let impostor = TestServer::start_on(port, generate_key(), None).await;
    let error = manager
        .open(request(vec![config], vec![password_secrets()], 1))
        .await
        .unwrap_err();
    assert!(matches!(error, SshError::HostKeyMismatch { .. }), "{error}");
    drop(impostor);
}

#[tokio::test]
async fn connections_share_one_session_and_forward() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(None).await;
    let (echo, _echo_task) = start_echo().await;
    let (other_echo, _other_task) = start_echo().await;
    let manager = manager(dir.path());
    let config = ssh_config("Homelab SSH", server.port, SshAuthMethod::Password);
    let unknown = manager
        .open(request(
            vec![config.clone()],
            vec![password_secrets()],
            echo,
        ))
        .await
        .unwrap_err();
    trust_all(&manager, unknown);

    let postgres = manager
        .open(request(
            vec![config.clone()],
            vec![password_secrets()],
            echo,
        ))
        .await
        .unwrap();
    let mariadb = manager
        .open(request(
            vec![config.clone()],
            vec![password_secrets()],
            echo,
        ))
        .await
        .unwrap();
    let redis = manager
        .open(request(vec![config], vec![password_secrets()], other_echo))
        .await
        .unwrap();
    assert_eq!(postgres.local_address(), mariadb.local_address());
    assert_ne!(postgres.local_address(), redis.local_address());
    let sessions = manager.sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].references, 3);

    let shared_port = postgres.local_address().port();
    drop(postgres);
    assert_eq!(
        echo_through(shared_port, b"still open").await,
        b"still open"
    );
    drop(mariadb);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        TcpStream::connect(("127.0.0.1", shared_port))
            .await
            .is_err()
    );
    assert_eq!(
        echo_through(redis.local_address().port(), b"redis").await,
        b"redis"
    );
    drop(redis);
    assert!(manager.sessions().is_empty());
}

#[tokio::test]
async fn jump_hosts_chain_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let gateway_a = TestServer::start(None).await;
    let gateway_b = TestServer::start(None).await;
    let (echo, _echo_task) = start_echo().await;
    let manager = manager(dir.path());
    let first = ssh_config("Gateway A", gateway_a.port, SshAuthMethod::Password);
    let mut second = ssh_config("Gateway B", gateway_b.port, SshAuthMethod::Password);
    second.jump_host = Some(first.id);
    let chain = vec![first, second];
    let secrets = vec![password_secrets(), password_secrets()];
    for _ in 0..2 {
        let error = manager
            .open(request(chain.clone(), secrets.clone(), echo))
            .await
            .unwrap_err();
        trust_all(&manager, error);
    }
    let lease = manager.open(request(chain, secrets, echo)).await.unwrap();
    assert_eq!(
        echo_through(lease.local_address().port(), b"two hops").await,
        b"two hops"
    );
}

#[tokio::test]
async fn tunnels_reconnect_after_the_server_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(None).await;
    let port = server.port;
    let host_key = server.host_key.clone();
    let (echo, _echo_task) = start_echo().await;
    let manager = manager(dir.path());
    let config = ssh_config("Bastion", port, SshAuthMethod::Password);
    let unknown = manager
        .open(request(
            vec![config.clone()],
            vec![password_secrets()],
            echo,
        ))
        .await
        .unwrap_err();
    trust_all(&manager, unknown);
    let lease = manager
        .open(request(vec![config], vec![password_secrets()], echo))
        .await
        .unwrap();
    let local = lease.local_address().port();
    assert_eq!(echo_through(local, b"before").await, b"before");

    server.shutdown().await;
    let mut lost = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if !lease.status().is_usable() {
            lost = true;
            break;
        }
    }
    assert!(lost, "the dropped session was not detected");
    let _restarted = TestServer::start_on(port, host_key, None).await;
    assert_eq!(echo_through(local, b"after").await, b"after");
    assert_eq!(lease.status(), TunnelStatus::Connected);
}

#[tokio::test]
async fn verification_connects_without_forwarding() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(None).await;
    let manager = manager(dir.path());
    let config = ssh_config("Bastion", server.port, SshAuthMethod::Password);
    let unknown = manager
        .verify(std::slice::from_ref(&config), &[password_secrets()])
        .await
        .unwrap_err();
    trust_all(&manager, unknown);
    let endpoint = manager
        .verify(&[config], &[password_secrets()])
        .await
        .unwrap();
    assert_eq!(endpoint, format!("127.0.0.1:{}", server.port));
    assert!(manager.sessions().is_empty());
}

#[tokio::test]
async fn occupied_local_ports_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(None).await;
    let manager = manager(dir.path());
    let config = ssh_config("Bastion", server.port, SshAuthMethod::Password);
    let unknown = manager
        .open(request(vec![config.clone()], vec![password_secrets()], 1))
        .await
        .unwrap_err();
    trust_all(&manager, unknown);
    let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = occupied.local_addr().unwrap().port();
    let mut conflicting = request(vec![config], vec![password_secrets()], 1);
    conflicting.local_port = Some(port);
    let error = manager.open(conflicting).await.unwrap_err();
    assert!(matches!(error, SshError::PortUnavailable { port: p, .. } if p == port));
    assert!(manager.sessions().is_empty());
}

#[tokio::test]
async fn unreachable_servers_fail_with_connection_errors() {
    let dir = tempfile::tempdir().unwrap();
    let free = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);
    let manager = manager(dir.path());
    let config = ssh_config("Offline", port, SshAuthMethod::Password);
    let error = manager
        .open(request(vec![config], vec![password_secrets()], 1))
        .await
        .unwrap_err();
    assert!(matches!(error, SshError::Connection { .. }), "{error}");
}
