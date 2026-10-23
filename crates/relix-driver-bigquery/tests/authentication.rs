/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE;
use common::*;
use jsonwebtoken::{Algorithm, DecodingKey};
use relix_core::TlsMode;
use relix_driver_api::{DatabaseDriver, ErrorKind};
use relix_driver_bigquery::BigQueryDriver;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn service_accounts_exchange_a_signed_jwt_for_a_bearer_token() {
    let server = MockServer::start().await;
    let connection = connect(&server, &[]).await;
    assert_eq!(connection.server_info().product, "Google BigQuery");

    let token_requests = requests_to(&server, "POST", "/token").await;
    assert_eq!(token_requests.len(), 1);
    let form: Vec<(String, String)> = url_form(&token_requests[0].body);
    let grant = form.iter().find(|(key, _)| key == "grant_type").unwrap();
    assert_eq!(grant.1, "urn:ietf:params:oauth:grant-type:jwt-bearer");
    let assertion = &form.iter().find(|(key, _)| key == "assertion").unwrap().1;
    let (signing_input, signature) = assertion.rsplit_once('.').unwrap();
    let verified = jsonwebtoken::crypto::verify(
        signature.trim_end_matches('='),
        signing_input.as_bytes(),
        &DecodingKey::from_rsa_der(&public_key_pkcs1_der()),
        Algorithm::RS256,
    )
    .unwrap();
    assert!(
        verified,
        "the assertion is signed with the service account key"
    );
    let payload = signing_input.split('.').nth(1).unwrap();
    let claims: serde_json::Value =
        serde_json::from_slice(&URL_SAFE.decode(payload).unwrap()).unwrap();
    assert_eq!(claims["aud"], format!("{}/token", server.uri()));
    assert_eq!(claims["iss"], CLIENT_EMAIL);
    assert_eq!(claims["scope"], "https://www.googleapis.com/auth/bigquery");

    let probes = requests_to(&server, "GET", &api("datasets")).await;
    assert_eq!(query_param(&probes[0], "maxResults").as_deref(), Some("1"));
    assert_eq!(
        header(&probes[0], "authorization"),
        format!("Bearer {ACCESS_TOKEN}")
    );
    connection.ping().await.unwrap();
    assert_eq!(
        requests_to(&server, "POST", "/token").await.len(),
        1,
        "tokens are cached until they expire"
    );
}

fn url_form(body: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(body);
    relix_driver_http::Url::parse(&format!("http://form/?{text}"))
        .unwrap()
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

#[tokio::test]
async fn rejected_tokens_are_authentication_errors() {
    let server = MockServer::start().await;
    mount_token_endpoint(&server).await;
    Mock::given(method("GET"))
        .and(path(api("datasets")))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": {
                "code": 401,
                "message": "Request had invalid authentication credentials.",
                "errors": [{"message": "Invalid Credentials", "domain": "global", "reason": "authError"}],
                "status": "UNAUTHENTICATED",
            }
        })))
        .mount(&server)
        .await;
    let key = service_account_file(&server);
    let error = BigQueryDriver::new()
        .connect(&config(
            &server,
            &[("credentials_file", key.to_str().unwrap())],
        ))
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication);
    assert!(!error.to_string().contains(ACCESS_TOKEN));
}

#[tokio::test]
async fn failing_token_endpoints_are_reported_as_authentication_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Invalid JWT Signature.",
        })))
        .mount(&server)
        .await;
    let key = service_account_file(&server);
    let error = BigQueryDriver::new()
        .connect(&config(
            &server,
            &[("credentials_file", key.to_str().unwrap())],
        ))
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication);
    assert!(
        requests_to(&server, "GET", &api("datasets"))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn emulator_endpoints_can_be_used_without_credentials() {
    let server = MockServer::start().await;
    mount_dataset_probe(&server).await;
    BigQueryDriver::new()
        .connect(&config(&server, &[("auth", "none")]))
        .await
        .unwrap();
    let probe = &requests_to(&server, "GET", &api("datasets")).await[0];
    assert_eq!(header(probe, "authorization"), "");
}

#[tokio::test]
async fn https_endpoints_are_never_downgraded() {
    let server = MockServer::start().await;
    mount_dataset_probe(&server).await;
    let mut config = config(&server, &[("auth", "none")]);
    config.settings.tls.mode = TlsMode::Require;
    let error = BigQueryDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Configuration);
    assert!(server.received_requests().await.unwrap().is_empty());
}
