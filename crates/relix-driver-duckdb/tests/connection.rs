/*
 * @author t0bx
 * @since 23.10.2026
 */

mod common;

use relix_core::{ConnectionConfig, ConnectionSettings, StatementOutcome, Value};
use relix_driver_api::{DatabaseDriver, ErrorKind, ExecuteOptions};
use relix_driver_duckdb::DuckDbDriver;
use std::path::PathBuf;

#[tokio::test]
async fn file_databases_are_created_and_persist_between_connections() {
    let database = common::file_database();
    let connection = common::connect(&database.config).await;
    let info = connection.server_info();
    assert_eq!(info.product, "DuckDB");
    assert!(info.version.starts_with('v'), "{}", info.version);
    connection.ping().await.unwrap();
    common::run(connection.as_ref(), "CREATE TABLE kept (a INTEGER)").await;
    common::run(connection.as_ref(), "INSERT INTO kept VALUES (7)").await;
    connection.close().await.unwrap();
    assert!(database.path().exists());

    let reopened = common::connect(&database.config).await;
    let batch = common::rows(reopened.as_ref(), "SELECT a FROM kept").await;
    assert_eq!(batch.rows, vec![vec![Value::Int(7)]]);
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn missing_files_are_not_created_implicitly() {
    let database = common::file_database();
    let mut config = database.config.clone();
    config.settings.create_if_missing = false;
    let error = DuckDbDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Configuration);
    assert!(!database.path().exists());
}

#[tokio::test]
async fn in_memory_databases_are_supported() {
    let settings = ConnectionSettings {
        file_path: Some(PathBuf::from(":memory:")),
        ..ConnectionSettings::default()
    };
    let connection = common::connect(&ConnectionConfig::new(settings, None)).await;
    common::run(connection.as_ref(), "CREATE TABLE t AS SELECT 1 AS a").await;
    let databases = common::sql(connection.as_ref()).databases().await.unwrap();
    assert!(
        databases
            .iter()
            .any(|database| database.name == "memory" && database.is_default)
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn read_only_access_rejects_writes() {
    let database = common::file_database();
    let writer = common::connect(&database.config).await;
    common::run(writer.as_ref(), "CREATE TABLE t (a INTEGER)").await;
    writer.close().await.unwrap();

    let mut config = database.config.clone();
    config
        .settings
        .properties
        .insert("access_mode".into(), "read_only".into());
    let reader = common::connect(&config).await;
    let error = common::sql(reader.as_ref())
        .execute("INSERT INTO t VALUES (1)", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    assert!(
        error.message.contains("read-only"),
        "unexpected message: {}",
        error.message
    );
    let outcome = common::run(reader.as_ref(), "SELECT count(*) FROM t").await;
    assert!(matches!(outcome, StatementOutcome::Rows(_)));
    reader.close().await.unwrap();
}

#[tokio::test]
async fn closed_connections_report_connection_errors() {
    let database = common::file_database();
    let connection = common::connect(&database.config).await;
    connection.close().await.unwrap();
    let error = connection.ping().await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Connection);
}
