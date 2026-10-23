/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use relix_core::TlsMode;
use relix_driver_api::{DatabaseDriver, ErrorKind};
use relix_driver_snowflake::SnowflakeDriver;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn server_answering_version() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/statements"))
        .respond_with(ResponseTemplate::new(200).set_body_json(version_result()))
        .mount(&server)
        .await;
    server
}

fn bearer(request: &wiremock::Request) -> String {
    header(request, "authorization")
        .strip_prefix("Bearer ")
        .expect("bearer token")
        .to_owned()
}

#[tokio::test]
async fn key_pair_jwt_carries_the_documented_claims() {
    let server = server_answering_version().await;
    let connection = SnowflakeDriver::new()
        .connect(&key_pair_config(&server))
        .await
        .unwrap();
    assert_eq!(connection.server_info().product, "Snowflake");
    assert_eq!(connection.server_info().version, "8.40.1");
    connection.ping().await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let request = &requests[0];
    assert_eq!(request.url.path(), "/api/v2/statements");
    assert!(query_param(request, "requestId").is_some());
    assert_eq!(query_param(request, "async"), None);
    assert_eq!(
        header(request, "x-snowflake-authorization-token-type"),
        "KEYPAIR_JWT"
    );
    assert_eq!(header(request, "accept"), "application/json");
    assert_eq!(header(request, "content-type"), "application/json");
    assert!(header(request, "user-agent").starts_with("relix/"));
    assert_eq!(
        body(request),
        json!({
            "statement": "SELECT CURRENT_VERSION()",
            "database": "SALES",
            "warehouse": "COMPUTE_WH",
            "role": "ANALYST_ROLE",
        })
    );

    let token = bearer(request);
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_required_spec_claims(&["exp", "iat", "iss", "sub"]);
    let decoded = decode::<serde_json::Value>(
        &token,
        &DecodingKey::from_rsa_der(&public_key_pkcs1_der()),
        &validation,
    )
    .unwrap();
    assert_eq!(decoded.header.alg, Algorithm::RS256);
    let claims = decoded.claims;
    assert_eq!(
        claims["iss"],
        format!("ACME.ANALYST.{}", expected_fingerprint())
    );
    assert_eq!(claims["sub"], "ACME.ANALYST");
    let lifetime = claims["exp"].as_u64().unwrap() - claims["iat"].as_u64().unwrap();
    assert_eq!(lifetime, 59 * 60);
    assert_eq!(
        bearer(&requests[1]),
        token,
        "tokens are reused until close to expiry"
    );
}

#[tokio::test]
async fn encrypted_private_keys_use_the_password_as_passphrase() {
    let server = server_answering_version().await;
    let path = write_key_file(true);
    let config = config(&server, &[("private_key_file", path.to_str().unwrap())]);
    let missing = SnowflakeDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(missing.kind, ErrorKind::Authentication);
    let wrong = SnowflakeDriver::new()
        .connect(&with_password(config.clone(), "not-the-passphrase-42"))
        .await
        .err()
        .unwrap();
    assert_eq!(wrong.kind, ErrorKind::Authentication);
    assert!(!wrong.to_string().contains("not-the-passphrase-42"));
    assert!(server.received_requests().await.unwrap().is_empty());

    SnowflakeDriver::new()
        .connect(&with_password(config, PASSPHRASE))
        .await
        .unwrap();
    let request = &server.received_requests().await.unwrap()[0];
    let claims: serde_json::Value = decode(
        &bearer(request),
        &DecodingKey::from_rsa_der(&public_key_pkcs1_der()),
        &Validation::new(Algorithm::RS256),
    )
    .unwrap()
    .claims;
    assert_eq!(claims["sub"], "ACME.ANALYST");
}

#[tokio::test]
async fn oauth_and_programmatic_tokens_are_sent_with_their_type() {
    for (authenticator, token_type) in [
        ("oauth", "OAUTH"),
        ("programmatic_access_token", "PROGRAMMATIC_ACCESS_TOKEN"),
    ] {
        let server = server_answering_version().await;
        let config = config(&server, &[("authenticator", authenticator)]);
        let error = SnowflakeDriver::new().connect(&config).await.err().unwrap();
        assert_eq!(error.kind, ErrorKind::Authentication);
        SnowflakeDriver::new()
            .connect(&with_password(config, "token-value"))
            .await
            .unwrap();
        let request = &server.received_requests().await.unwrap()[0];
        assert_eq!(bearer(request), "token-value");
        assert_eq!(
            header(request, "x-snowflake-authorization-token-type"),
            token_type
        );
    }
}

#[tokio::test]
async fn password_logins_are_rejected_before_any_request() {
    let server = server_answering_version().await;
    let config = with_password(
        config(&server, &[("authenticator", "snowflake")]),
        "secret-password",
    );
    let error = SnowflakeDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert!(error.to_string().contains("does not accept password"));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn rejected_jwts_are_authentication_errors_without_secrets() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/statements"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "code": "390144",
            "message": "JWT token is invalid. [0f6a2c1e-4f3b-4a4e-9c2e-0d9e1c0b6c11]",
        })))
        .mount(&server)
        .await;
    let error = SnowflakeDriver::new()
        .connect(&key_pair_config(&server))
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication);
    assert!(error.message.starts_with("JWT token is invalid"));
    let token = bearer(&server.received_requests().await.unwrap()[0]);
    assert!(!error.to_string().contains(&token));
}

#[tokio::test]
async fn https_is_never_downgraded_to_plain_http() {
    let server = server_answering_version().await;
    let mut config = key_pair_config(&server);
    config.settings.tls.mode = TlsMode::Prefer;
    let error = SnowflakeDriver::new().connect(&config).await.err().unwrap();
    assert!(matches!(
        error.kind,
        ErrorKind::Tls | ErrorKind::Connection | ErrorKind::Io | ErrorKind::Protocol
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}
