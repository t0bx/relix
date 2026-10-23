/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used)]

use relix_core::{ConnectionConfig, ConnectionSettings, Secret, TlsMode, TlsSettings};
use relix_driver_api::ErrorKind;
use relix_driver_http::{EndpointOptions, HttpAuth, HttpEndpoint, Method, Url};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn serve(status: &'static str, body: &'static str) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = requests.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = vec![0u8; 8192];
            let read = socket.read(&mut buffer).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            if !request.starts_with("GET") && !request.starts_with("POST") {
                continue;
            }
            seen.lock().unwrap().push(request);
            let response = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    (port, requests)
}

fn config(port: u16, mode: TlsMode) -> ConnectionConfig {
    let settings = ConnectionSettings {
        host: Some("127.0.0.1".into()),
        port: Some(port),
        username: Some("admin".into()),
        tls: TlsSettings {
            mode,
            ca_certificate: None,
        },
        connect_timeout_secs: 3,
        ..ConnectionSettings::default()
    };
    ConnectionConfig::new(settings, Some(Secret::new("pw")))
}

fn options(config: &ConnectionConfig) -> EndpointOptions {
    EndpointOptions {
        default_port: 9200,
        probe_path: "/".into(),
        auth: HttpAuth::basic_from(config),
        request_timeout: None,
    }
}

#[tokio::test]
async fn plain_http_with_basic_auth() {
    let (port, requests) = serve("200 OK", r#"{"ok":true}"#).await;
    let config = config(port, TlsMode::Disable);
    let endpoint = HttpEndpoint::connect(&config, options(&config))
        .await
        .unwrap();
    assert!(!endpoint.is_https());
    let value: serde_json::Value = endpoint
        .json(endpoint.request(Method::GET, "/_cluster/health"))
        .await
        .unwrap();
    assert_eq!(value["ok"], true);
    let last = requests.lock().unwrap().last().cloned().unwrap();
    assert!(last.starts_with("GET /_cluster/health"));
    assert!(last.to_lowercase().contains("authorization: basic"));
}

#[tokio::test]
async fn prefer_falls_back_to_http_when_tls_is_unavailable() {
    let (port, _) = serve("200 OK", "{}").await;
    let config = config(port, TlsMode::Prefer);
    let endpoint = HttpEndpoint::connect(&config, options(&config))
        .await
        .unwrap();
    assert!(!endpoint.is_https());
}

#[tokio::test]
async fn require_never_falls_back() {
    let (port, _) = serve("200 OK", "{}").await;
    let config = config(port, TlsMode::Require);
    let error = HttpEndpoint::connect(&config, options(&config))
        .await
        .err()
        .unwrap();
    assert!(matches!(
        error.kind,
        ErrorKind::Tls | ErrorKind::Connection | ErrorKind::Io | ErrorKind::Protocol
    ));
}

#[tokio::test]
async fn rejected_credentials_are_authentication_errors() {
    let (port, _) = serve(
        "401 Unauthorized",
        r#"{"error":{"reason":"missing authentication"}}"#,
    )
    .await;
    let config = config(port, TlsMode::Disable);
    let error = HttpEndpoint::connect(&config, options(&config))
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication);
}

#[tokio::test]
async fn forwarded_endpoints_keep_the_original_host_name() {
    let (port, requests) = serve("200 OK", "{}").await;
    let mut original = config(9200, TlsMode::Disable);
    original.settings.host = Some("search.internal".into());
    let tunneled = original.redirected("127.0.0.1", port);
    let endpoint = HttpEndpoint::connect(&tunneled, options(&tunneled))
        .await
        .unwrap();
    assert_eq!(endpoint.base_url().host_str(), Some("search.internal"));
    let request = requests.lock().unwrap()[0].to_lowercase();
    assert!(request.contains(&format!("host: search.internal:{port}")));
}

#[tokio::test]
async fn fixed_base_urls_accept_plain_http_only_with_tls_disabled() {
    let (port, requests) = serve("404 Not Found", r#"{"error":"missing"}"#).await;
    let base = Url::parse(&format!("http://127.0.0.1:{port}/api")).unwrap();
    let refused = HttpEndpoint::at_base_url(
        &config(port, TlsMode::Prefer),
        base.clone(),
        HttpAuth::None,
        None,
    )
    .await
    .err()
    .unwrap();
    assert_eq!(refused.kind, ErrorKind::Configuration);
    let endpoint = HttpEndpoint::at_base_url(
        &config(port, TlsMode::Disable),
        base,
        HttpAuth::Bearer(Secret::new("token")),
        None,
    )
    .await
    .unwrap();
    let url = endpoint.segments_url(&["datasets", "my table/1"]);
    assert_eq!(url.path(), "/api/datasets/my%20table%2F1");
    let response = endpoint
        .exchange(endpoint.request_url(Method::GET, url))
        .await
        .unwrap();
    assert_eq!(response.status.as_u16(), 404);
    assert_eq!(response.body, r#"{"error":"missing"}"#);
    let request = requests.lock().unwrap()[0].to_lowercase();
    assert!(request.starts_with("get /api/datasets/my%20table%2f1"));
    assert!(request.contains("authorization: bearer token"));
}

#[tokio::test]
async fn fixed_base_urls_follow_ssh_tunnels() {
    let (port, requests) = serve("200 OK", "{}").await;
    let mut original = config(443, TlsMode::Disable);
    original.settings.host = Some("acme.example.com".into());
    let tunneled = original.redirected("127.0.0.1", port);
    let base = Url::parse(&format!("http://acme.example.com:{port}/")).unwrap();
    let endpoint = HttpEndpoint::at_base_url(&tunneled, base, HttpAuth::None, None)
        .await
        .unwrap();
    endpoint
        .send(endpoint.request(Method::POST, "/api/v2/statements"))
        .await
        .unwrap();
    let request = requests.lock().unwrap()[0].to_lowercase();
    assert!(request.contains(&format!("host: acme.example.com:{port}")));
}
