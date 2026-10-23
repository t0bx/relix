/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{Secret, TlsMode};
use relix_driver_api::{DatabaseDriver, ErrorKind, ExecuteOptions};
use relix_driver_oracle::OracleDriver;

#[tokio::test]
async fn connects_and_reports_server_info() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let info = connection.server_info();
    assert_eq!(info.product, "Oracle Database");
    assert!(info.version.starts_with("23"), "{}", info.version);
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
    let sql = common::sql(connection.as_ref());
    let error = sql
        .execute("SELECT 1 FROM dual", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Connection);
    assert!(sql.schemas("FREEPDB1").await.is_err());
}

#[tokio::test]
async fn wrong_password_is_an_authentication_error() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.password = Some(Secret::new("definitely-wrong"));
    let error = OracleDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication, "{error}");
    assert!(!error.to_string().contains("definitely-wrong"));
}

#[tokio::test]
async fn unreachable_port_and_unknown_service_are_reported() {
    let Some(config) = common::config() else {
        return;
    };
    let mut closed_port = config.clone();
    closed_port.settings.port = Some(1);
    let error = OracleDriver::new()
        .connect(&closed_port)
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Connection, "{error}");
    let mut unknown_service = config;
    unknown_service.settings.database = Some("NO_SUCH_SERVICE".into());
    let error = OracleDriver::new()
        .connect(&unknown_service)
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Configuration, "{error}");
}

#[tokio::test]
async fn enforced_tls_is_rejected_and_prefer_falls_back() {
    let Some(mut config) = common::config() else {
        return;
    };
    for mode in [TlsMode::Require, TlsMode::VerifyFull] {
        config.settings.tls.mode = mode;
        let error = OracleDriver::new().connect(&config).await.err().unwrap();
        assert_eq!(error.kind, ErrorKind::Tls, "{error}");
    }
    config.settings.tls.mode = TlsMode::Prefer;
    let connection = common::connect(&config).await;
    connection.close().await.unwrap();
}
