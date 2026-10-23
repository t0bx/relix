/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used)]

mod common;

use common::{Engine, for_each_engine};
use relix_core::{Secret, TlsMode};
use relix_driver_api::{ErrorKind, ExecuteOptions};

for_each_engine!(
    connects_and_reports_server_info,
    wrong_password_is_an_authentication_error,
    wrong_port_is_a_connection_error,
    closed_connections_reject_further_work,
    prefer_falls_back_and_require_demands_tls,
);

async fn connects_and_reports_server_info(engine: Engine) {
    let Some(config) = engine.config() else {
        return;
    };
    let connection = common::connect(engine, &config).await;
    let info = connection.server_info();
    assert_eq!(info.product, engine.product());
    assert!(!info.version.is_empty());
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
}

async fn wrong_password_is_an_authentication_error(engine: Engine) {
    let Some(mut config) = engine.config() else {
        return;
    };
    if !engine.checks_passwords() {
        return;
    }
    config.password = Some(Secret::new("definitely-wrong"));
    let error = engine.driver().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication, "{error}");
    assert!(!error.message.contains("definitely-wrong"));
}

async fn wrong_port_is_a_connection_error(engine: Engine) {
    let Some(mut config) = engine.config() else {
        return;
    };
    config.settings.port = Some(1);
    let error = engine.driver().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Connection, "{error}");
}

async fn closed_connections_reject_further_work(engine: Engine) {
    let Some(config) = engine.config() else {
        return;
    };
    let connection = common::connect(engine, &config).await;
    connection.close().await.unwrap();
    let sql = common::sql(connection.as_ref());
    let error = sql
        .execute("SELECT 1", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Connection);
    assert!(sql.databases().await.is_err());
}

async fn prefer_falls_back_and_require_demands_tls(engine: Engine) {
    let Some(mut config) = engine.config() else {
        return;
    };
    config.settings.tls.mode = TlsMode::Prefer;
    let connection = common::connect(engine, &config).await;
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
    config.settings.tls.mode = TlsMode::Require;
    match engine.driver().connect(&config).await {
        Ok(connection) => connection.close().await.unwrap(),
        Err(error) => assert_eq!(error.kind, ErrorKind::Tls, "{error}"),
    }
}
