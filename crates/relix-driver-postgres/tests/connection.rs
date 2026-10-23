/*
 * @author t0bx
 * @since 23.10.2026
 */

mod common;

use relix_core::{Secret, TlsMode};
use relix_driver_api::{DatabaseDriver, ErrorKind, ExecuteOptions};
use relix_driver_postgres::PostgresDriver;

#[tokio::test]
async fn connects_and_reports_server_info() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let info = connection.server_info();
    assert_eq!(info.product, "PostgreSQL");
    assert!(!info.version.is_empty());
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
async fn wrong_password_is_an_authentication_error() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.password = Some(Secret::new("definitely-wrong"));
    let error = PostgresDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication, "{error}");
    assert!(!error.message.contains("definitely-wrong"));
}

#[tokio::test]
async fn wrong_port_is_a_connection_error() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.settings.port = Some(1);
    let error = PostgresDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Connection, "{error}");
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
        .execute("SELECT 1", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Connection);
    assert!(sql.databases().await.is_err());
}

#[tokio::test]
async fn prefer_falls_back_and_require_demands_tls() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.settings.tls.mode = TlsMode::Prefer;
    let connection = common::connect(&config).await;
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
    config.settings.tls.mode = TlsMode::Require;
    match PostgresDriver::new().connect(&config).await {
        Ok(connection) => connection.close().await.unwrap(),
        Err(error) => assert_eq!(error.kind, ErrorKind::Tls, "{error}"),
    }
}
