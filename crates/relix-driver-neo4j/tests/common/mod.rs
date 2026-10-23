/*
 * @author t0bx
 * @since 24.10.2026
 */

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use relix_core::{
    ConnectionConfig, ConnectionSettings, RowBatch, Secret, StatementOutcome, TlsMode, TlsSettings,
};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, ExecuteOptions, SqlConnection};
use relix_driver_neo4j::Neo4jDriver;
use relix_testing::integration::{integration_enabled, unique_name};
use std::env;

/// Neo4j started as `neo4j:5` with `NEO4J_AUTH=neo4j/relix_test_pw` on port 57687;
/// override with `RELIX_TEST_NEO4J_HOST/_PORT/_USER/_PASSWORD`.
pub fn config() -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping Neo4j integration test: set RELIX_INTEGRATION=1");
        return None;
    }
    let read = |suffix: &str| env::var(format!("RELIX_TEST_NEO4J_{suffix}")).ok();
    let settings = ConnectionSettings {
        host: Some(read("HOST").unwrap_or_else(|| "127.0.0.1".to_owned())),
        port: Some(
            read("PORT")
                .and_then(|port| port.parse().ok())
                .unwrap_or(57687),
        ),
        username: Some(read("USER").unwrap_or_else(|| "neo4j".to_owned())),
        connect_timeout_secs: 15,
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        ..ConnectionSettings::default()
    };
    let password = read("PASSWORD").unwrap_or_else(|| "relix_test_pw".to_owned());
    Some(ConnectionConfig::new(settings, Some(Secret::new(password))))
}

pub async fn connect(config: &ConnectionConfig) -> Box<dyn DatabaseConnection> {
    Neo4jDriver::new()
        .connect(config)
        .await
        .unwrap_or_else(|error| panic!("connect failed: {error}"))
}

pub fn sql(connection: &dyn DatabaseConnection) -> &dyn SqlConnection {
    connection
        .as_sql()
        .unwrap_or_else(|| panic!("Neo4j connections expose the tabular interface"))
}

pub async fn run(connection: &dyn DatabaseConnection, statement: &str) -> StatementOutcome {
    sql(connection)
        .execute(statement, &ExecuteOptions::default())
        .await
        .unwrap_or_else(|error| panic!("{statement} failed: {error}"))
}

pub async fn rows(connection: &dyn DatabaseConnection, statement: &str) -> RowBatch {
    match run(connection, statement).await {
        StatementOutcome::Rows(batch) => batch,
        other => panic!("expected rows from {statement}, got {other:?}"),
    }
}

pub fn label(prefix: &str) -> String {
    unique_name(prefix)
}

pub async fn remove_label(connection: &dyn DatabaseConnection, label: &str) {
    run(connection, &format!("MATCH (n:`{label}`) DETACH DELETE n")).await;
}

pub fn column(batch: &RowBatch, name: &str) -> usize {
    batch
        .column_index(name)
        .unwrap_or_else(|| panic!("column {name} missing in {:?}", batch.columns))
}
