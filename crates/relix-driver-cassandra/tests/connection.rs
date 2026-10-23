/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::Engine;
use relix_core::{Secret, TlsMode};
use relix_driver_api::{ErrorKind, ExecuteOptions};

#[tokio::test]
async fn connects_and_detects_the_product() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let info = connection.server_info();
    assert_eq!(info.product, Engine::current().product());
    assert!(!info.version.is_empty() && info.version != "unknown");
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
async fn wrong_password_is_an_authentication_error() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.password = Some(Secret::new("definitely-wrong"));
    let error = Engine::current().driver().connect(&config).await.err().unwrap();
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
    let error = Engine::current().driver().connect(&config).await.err().unwrap();
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
    let error = Engine::current().driver().connect(&config).await.err().unwrap();
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
        .execute("SELECT now() FROM system.local", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Connection);
    assert!(sql.databases().await.is_err());
    assert!(connection.ping().await.is_err());
}

#[tokio::test]
async fn configured_keyspace_becomes_the_session_default() {
    let Some(config) = common::config() else {
        return;
    };
    let setup = common::connect(&config).await;
    let keyspace = common::create_keyspace(setup.as_ref()).await;
    common::run(
        setup.as_ref(),
        &format!("CREATE TABLE {keyspace}.notes (id int PRIMARY KEY, body text)"),
    )
    .await;
    let mut scoped = config.clone();
    scoped.settings.database = Some(keyspace.clone());
    let connection = common::connect(&scoped).await;
    common::run(
        connection.as_ref(),
        "INSERT INTO notes (id, body) VALUES (1, 'x')",
    )
    .await;
    let databases = common::sql(connection.as_ref()).databases().await.unwrap();
    let default = databases.iter().find(|database| database.is_default).unwrap();
    assert_eq!(default.name, keyspace);
    let mut missing = config.clone();
    missing.settings.database = Some("relix_missing_keyspace".into());
    let error = Engine::current().driver().connect(&missing).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Configuration, "{error}");
    connection.close().await.unwrap();
    common::drop_keyspace(setup.as_ref(), &keyspace).await;
    setup.close().await.unwrap();
}
