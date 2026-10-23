/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use relix_core::{ConnectionConfig, ConnectionSettings, TlsMode, TlsSettings};
use relix_driver_api::{DatabaseConnection, DatabaseDriver};
use relix_driver_bigquery::BigQueryDriver;
use relix_testing::rsa_keys::TEST_RSA_PRIVATE_KEY_PEM;
use rsa::RsaPrivateKey;
use rsa::pkcs1::EncodeRsaPublicKey;
use rsa::pkcs8::DecodePrivateKey;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::OnceLock;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

pub const PROJECT: &str = "acme";
pub const ACCESS_TOKEN: &str = "ya29.relix-test-token";
pub const CLIENT_EMAIL: &str = "relix-test@acme.iam.gserviceaccount.com";

pub fn api(path: &str) -> String {
    format!("/bigquery/v2/projects/{PROJECT}/{path}")
}

pub fn private_key() -> &'static RsaPrivateKey {
    static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    KEY.get_or_init(|| RsaPrivateKey::from_pkcs8_pem(TEST_RSA_PRIVATE_KEY_PEM).unwrap())
}

pub fn public_key_pkcs1_der() -> Vec<u8> {
    private_key()
        .to_public_key()
        .to_pkcs1_der()
        .unwrap()
        .as_bytes()
        .to_vec()
}

/// Writes a service account key file in Google's JSON format whose token endpoint is the
/// mock server, so the complete JWT-bearer token exchange runs locally.
pub fn service_account_file(server: &MockServer) -> PathBuf {
    let key = json!({
        "type": "service_account",
        "project_id": PROJECT,
        "private_key_id": "0123456789abcdef",
        "private_key": TEST_RSA_PRIVATE_KEY_PEM,
        "client_email": CLIENT_EMAIL,
        "client_id": "1234567890",
        "token_uri": format!("{}/token", server.uri()),
    });
    let path = std::env::temp_dir().join(format!("relix-bigquery-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&path, key.to_string()).unwrap();
    path
}

pub async fn mount_token_endpoint(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": ACCESS_TOKEN,
            "expires_in": 3599,
            "token_type": "Bearer",
        })))
        .mount(server)
        .await;
}

pub async fn mount_dataset_probe(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(api("datasets")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "kind": "bigquery#datasetList",
            "datasets": [{"datasetReference": {"projectId": PROJECT, "datasetId": "shop"}, "location": "EU"}],
        })))
        .with_priority(10)
        .mount(server)
        .await;
}

pub fn config(server: &MockServer, properties: &[(&str, &str)]) -> ConnectionConfig {
    let mut settings = ConnectionSettings {
        connect_timeout_secs: 10,
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        ..ConnectionSettings::default()
    };
    settings.properties.insert("endpoint".into(), server.uri());
    settings
        .properties
        .insert("project_id".into(), PROJECT.into());
    for (name, value) in properties {
        settings
            .properties
            .insert((*name).to_owned(), (*value).to_owned());
    }
    ConnectionConfig::new(settings, None)
}

/// Connects with a service account whose tokens come from the mock token endpoint.
pub async fn connect(
    server: &MockServer,
    properties: &[(&str, &str)],
) -> Box<dyn DatabaseConnection> {
    mount_token_endpoint(server).await;
    mount_dataset_probe(server).await;
    let key = service_account_file(server);
    let mut all = vec![("credentials_file", key.to_str().unwrap())];
    all.extend_from_slice(properties);
    BigQueryDriver::new()
        .connect(&config(server, &all))
        .await
        .unwrap()
}

pub fn field(name: &str, field_type: &str) -> Value {
    json!({"name": name, "type": field_type, "mode": "NULLABLE"})
}

pub fn row(values: &[Value]) -> Value {
    json!({"f": values.iter().map(|value| json!({"v": value})).collect::<Vec<_>>()})
}

/// A `QueryResponse`/`GetQueryResultsResponse` body shaped like the REST reference.
pub fn query_response(job_id: &str, fields: Value, rows: Vec<Value>, total: u64) -> Value {
    json!({
        "kind": "bigquery#queryResponse",
        "schema": {"fields": fields},
        "jobReference": {"projectId": PROJECT, "jobId": job_id, "location": "EU"},
        "totalRows": total.to_string(),
        "rows": rows,
        "totalBytesProcessed": "0",
        "jobComplete": true,
        "cacheHit": false,
    })
}

pub fn running_job(job_id: &str) -> Value {
    json!({
        "kind": "bigquery#queryResponse",
        "jobReference": {"projectId": PROJECT, "jobId": job_id, "location": "EU"},
        "jobComplete": false,
    })
}

pub fn body(request: &Request) -> Value {
    serde_json::from_slice(&request.body).unwrap()
}

pub fn header<'a>(request: &'a Request, name: &str) -> &'a str {
    request
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

pub fn query_param(request: &Request, name: &str) -> Option<String> {
    request
        .url
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

pub async fn requests_to(server: &MockServer, method: &str, path: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.method.as_str() == method && request.url.path() == path)
        .collect()
}
