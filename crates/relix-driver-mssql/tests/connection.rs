/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{Secret, TlsMode, Value};
use relix_driver_api::{DatabaseDriver, ErrorKind, ExecuteOptions};
use relix_driver_mssql::MsSqlDriver;

const ENCRYPTION_SQL: &str =
    "SELECT encrypt_option FROM sys.dm_exec_connections WHERE session_id = @@SPID";

#[tokio::test]
async fn connects_and_reports_server_info() {
    let Some(config) = common::config().await else {
        return;
    };
    let connection = common::connect(&config).await;
    let info = connection.server_info();
    assert!(info.product.contains("SQL"), "{}", info.product);
    assert!(!info.version.is_empty());
    connection.ping().await.unwrap();
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
async fn wrong_password_is_an_authentication_error() {
    let Some(mut config) = common::config().await else {
        return;
    };
    config.password = Some(Secret::new("definitely-wrong"));
    let error = MsSqlDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication, "{error}");
    assert!(!error.to_string().contains("definitely-wrong"));
}

#[tokio::test]
async fn wrong_port_is_a_connection_error() {
    let Some(mut config) = common::config().await else {
        return;
    };
    config.settings.port = Some(1);
    let error = MsSqlDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Connection, "{error}");
}

#[tokio::test]
async fn encryption_follows_the_tls_mode() {
    let Some(mut config) = common::config().await else {
        return;
    };
    for (mode, expected) in [
        (TlsMode::Disable, "FALSE"),
        (TlsMode::Prefer, "TRUE"),
        (TlsMode::Require, "TRUE"),
    ] {
        config.settings.tls.mode = mode;
        let connection = common::connect(&config).await;
        let batch = common::rows_of(common::run(connection.as_ref(), ENCRYPTION_SQL).await);
        assert_eq!(batch.rows[0][0], Value::Text(expected.into()), "{mode:?}");
        connection.close().await.unwrap();
    }
    config.settings.tls.mode = TlsMode::VerifyFull;
    let error = MsSqlDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Tls, "{error}");
}

#[tokio::test]
async fn tunneled_connections_use_the_original_host_for_tls() {
    let Some(config) = common::config().await else {
        return;
    };
    let host = config.host_or_default().to_owned();
    let port = config.settings.port.unwrap();
    let mut original = config.clone();
    original.settings.host = Some("sql.relix.invalid".into());
    let mut tunneled = original.redirected(&host, port);
    tunneled.settings.tls.mode = TlsMode::Require;
    let connection = common::connect(&tunneled).await;
    let batch = common::rows_of(common::run(connection.as_ref(), ENCRYPTION_SQL).await);
    assert_eq!(batch.rows[0][0], Value::Text("TRUE".into()));
    connection.close().await.unwrap();

    tunneled.settings.tls.mode = TlsMode::VerifyFull;
    let error = MsSqlDriver::new().connect(&tunneled).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Tls, "{error}");
}
