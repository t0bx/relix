/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code, clippy::expect_used)]

use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::config::{BehaviorVersion, Credentials, Region};
use aws_smithy_http_client::Builder;
use aws_smithy_http_client::tls::Provider;
use aws_smithy_http_client::tls::rustls_provider::CryptoMode;
use relix_core::{ConnectionConfig, ConnectionSettings, Secret};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, DocumentConnection, QueryExecutor};
use relix_driver_dynamodb::DynamoDbDriver;
use relix_testing::integration::integration_enabled;
use std::env;

const PREFIX: &str = "RELIX_TEST_DYNAMODB";

fn read(suffix: &str) -> Option<String> {
    env::var(format!("{PREFIX}_{suffix}")).ok()
}

pub fn endpoint() -> String {
    read("ENDPOINT").unwrap_or_else(|| "http://127.0.0.1:58000".to_owned())
}

pub fn region() -> String {
    read("REGION").unwrap_or_else(|| "eu-central-1".to_owned())
}

fn access_key() -> String {
    read("ACCESS_KEY").unwrap_or_else(|| "relixtest".to_owned())
}

fn secret_key() -> String {
    read("SECRET_KEY").unwrap_or_else(|| "relix_test".to_owned())
}

/// Settings for the `dynamodb` service of `docker-compose.test.yml` (DynamoDB Local on
/// port 58000 with `-sharedDb`), overridable through `RELIX_TEST_DYNAMODB_*`. The access
/// key pair goes through the username/password fields.
pub fn dynamodb_config() -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping DynamoDB integration test: set RELIX_INTEGRATION=1");
        return None;
    }
    let mut settings = ConnectionSettings {
        username: Some(access_key()),
        connect_timeout_secs: 10,
        ..ConnectionSettings::default()
    };
    settings.properties.insert("region".into(), region());
    settings
        .properties
        .insert("endpoint_url".into(), endpoint());
    Some(ConnectionConfig::new(
        settings,
        Some(Secret::new(secret_key())),
    ))
}

pub async fn connect_with(config: &ConnectionConfig) -> Box<dyn DatabaseConnection> {
    DynamoDbDriver::new()
        .connect(config)
        .await
        .expect("connect")
}

pub async fn connect() -> Option<Box<dyn DatabaseConnection>> {
    let config = dynamodb_config()?;
    Some(connect_with(&config).await)
}

pub fn documents(connection: &dyn DatabaseConnection) -> &dyn DocumentConnection {
    connection.as_document().expect("document connection")
}

pub fn console(connection: &dyn DatabaseConnection) -> &dyn QueryExecutor {
    connection.as_query().expect("query executor")
}

/// A plain SDK client for fixtures the driver does not offer, such as creating tables.
pub fn admin_client() -> Client {
    let http_client = Builder::new()
        .tls_provider(Provider::Rustls(CryptoMode::Ring))
        .build_https();
    let config = aws_sdk_dynamodb::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new(region()))
        .endpoint_url(endpoint())
        .credentials_provider(Credentials::new(
            access_key(),
            secret_key(),
            None,
            None,
            "relix-tests",
        ))
        .http_client(http_client)
        .build();
    Client::from_conf(config)
}
