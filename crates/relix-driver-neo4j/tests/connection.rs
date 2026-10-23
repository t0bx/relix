/*
 * @author t0bx
 * @since 24.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{Secret, TlsMode};
use relix_driver_api::{DatabaseDriver, ErrorKind, ExecuteOptions};
use relix_driver_neo4j::Neo4jDriver;

#[tokio::test]
async fn connects_and_reports_the_server() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let info = connection.server_info();
    assert_eq!(info.product, "Neo4j");
    assert!(info.version.starts_with('5'), "{}", info.version);
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
async fn wrong_password_is_an_authentication_error() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.password = Some(Secret::new("definitely-wrong"));
    let error = Neo4jDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication, "{error}");
    assert!(!error.to_string().contains("definitely-wrong"));
}

#[tokio::test]
async fn unreachable_port_is_a_connection_error() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.settings.port = Some(1);
    config.settings.connect_timeout_secs = 5;
    let error = Neo4jDriver::new().connect(&config).await.err().unwrap();
    assert!(
        matches!(error.kind, ErrorKind::Connection | ErrorKind::Timeout),
        "{error}"
    );
}

#[tokio::test]
async fn tls_modes_are_honoured_against_a_plain_text_server() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.settings.tls.mode = TlsMode::Prefer;
    let connection = common::connect(&config).await;
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
    config.settings.tls.mode = TlsMode::Require;
    let error = Neo4jDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Tls, "{error}");
}

#[tokio::test]
async fn closed_connections_reject_further_work() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    connection.close().await.unwrap();
    let sql = common::sql(connection.as_ref());
    let error = sql
        .execute("RETURN 1", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Connection);
    assert!(sql.databases().await.is_err());
}

#[tokio::test]
async fn unknown_configured_database_is_rejected() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.settings.database = Some("relixmissingdb".into());
    let error = Neo4jDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Query, "{error}");
}
