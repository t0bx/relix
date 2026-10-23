/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

use relix_core::{ConnectionConfig, ConnectionSettings, TlsMode, TlsSettings};
use relix_driver_api::ErrorKind;
use relix_tls_relay::open_endpoint;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

struct TestCertificate {
    pem: String,
    der: Vec<u8>,
    key: Vec<u8>,
}

fn certificate() -> TestCertificate {
    let generated = rcgen::generate_simple_self_signed(vec!["db.example.test".to_owned()]).unwrap();
    TestCertificate {
        pem: generated.cert.pem(),
        der: generated.cert.der().to_vec(),
        key: generated.signing_key.serialize_der(),
    }
}

async fn echo(mut stream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin) {
    let mut buffer = [0u8; 64];
    while let Ok(read) = stream.read(&mut buffer).await {
        if read == 0 || stream.write_all(&buffer[..read]).await.is_err() {
            return;
        }
    }
}

async fn plain_echo_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(echo(stream));
        }
    });
    address
}

async fn tls_echo_server(certificate: &TestCertificate) -> SocketAddr {
    let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.der.clone().into()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(certificate.key.clone())),
        )
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(tls) = acceptor.accept(stream).await {
                    echo(tls).await;
                }
            });
        }
    });
    address
}

fn config(address: SocketAddr, mode: TlsMode, ca_certificate: Option<PathBuf>) -> ConnectionConfig {
    let settings = ConnectionSettings {
        host: Some(address.ip().to_string()),
        port: Some(address.port()),
        connect_timeout_secs: 3,
        tls: TlsSettings {
            mode,
            ca_certificate,
        },
        ..ConnectionSettings::default()
    };
    ConnectionConfig::new(settings, None)
}

fn write_ca(certificate: &TestCertificate) -> PathBuf {
    let path = std::env::temp_dir().join(format!("relix-relay-ca-{}.pem", std::process::id()));
    std::fs::write(&path, &certificate.pem).unwrap();
    path
}

async fn round_trip(address: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream.write_all(b"ping").await.unwrap();
    let mut answer = [0u8; 4];
    stream.read_exact(&mut answer).await.unwrap();
    answer.to_vec()
}

#[tokio::test]
async fn disabled_tls_connects_directly() {
    let server = plain_echo_server().await;
    let endpoint = open_endpoint(&config(server, TlsMode::Disable, None), 1)
        .await
        .unwrap();
    assert!(!endpoint.is_encrypted());
    assert_eq!(endpoint.port(), server.port());
    assert_eq!(round_trip(&endpoint.address()).await, b"ping");
}

#[tokio::test]
async fn prefer_falls_back_to_plain_text_servers() {
    let server = plain_echo_server().await;
    let endpoint = open_endpoint(&config(server, TlsMode::Prefer, None), 1)
        .await
        .unwrap();
    assert!(!endpoint.is_encrypted());
}

#[tokio::test]
async fn require_fails_against_plain_text_servers() {
    let server = plain_echo_server().await;
    let error = open_endpoint(&config(server, TlsMode::Require, None), 1)
        .await
        .err()
        .unwrap();
    assert!(
        matches!(error.kind, ErrorKind::Tls | ErrorKind::Timeout),
        "{error}"
    );
}

#[tokio::test]
async fn require_relays_traffic_over_tls() {
    let certificate = certificate();
    let server = tls_echo_server(&certificate).await;
    let endpoint = open_endpoint(&config(server, TlsMode::Require, None), 1)
        .await
        .unwrap();
    assert!(endpoint.is_encrypted());
    assert_eq!(endpoint.host(), "127.0.0.1");
    assert_ne!(endpoint.port(), server.port());
    assert_eq!(round_trip(&endpoint.address()).await, b"ping");
    assert_eq!(round_trip(&endpoint.address()).await, b"ping");
}

#[tokio::test]
async fn verify_full_checks_the_original_host_through_tunnels() {
    let certificate = certificate();
    let server = tls_echo_server(&certificate).await;
    let ca = write_ca(&certificate);
    let original = ConnectionConfig::new(
        ConnectionSettings {
            host: Some("db.example.test".into()),
            port: Some(9042),
            connect_timeout_secs: 3,
            tls: TlsSettings {
                mode: TlsMode::VerifyFull,
                ca_certificate: Some(ca.clone()),
            },
            ..ConnectionSettings::default()
        },
        None,
    );
    let tunneled = original.redirected("127.0.0.1", server.port());
    let endpoint = open_endpoint(&tunneled, 1).await.unwrap();
    assert_eq!(round_trip(&endpoint.address()).await, b"ping");

    let mut wrong_name = tunneled.clone();
    wrong_name.tls_server_name = Some("other.example.test".into());
    let error = open_endpoint(&wrong_name, 1).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Tls, "{error}");

    let direct = config(server, TlsMode::VerifyFull, Some(ca.clone()));
    let error = open_endpoint(&direct, 1).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Tls, "{error}");
    let _ = std::fs::remove_file(ca);
}

#[tokio::test]
async fn dropping_the_endpoint_stops_the_relay() {
    let certificate = certificate();
    let server = tls_echo_server(&certificate).await;
    let endpoint = open_endpoint(&config(server, TlsMode::Require, None), 1)
        .await
        .unwrap();
    let address = endpoint.address();
    drop(endpoint);
    tokio::task::yield_now().await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(TcpStream::connect(&address).await.is_err());
}
