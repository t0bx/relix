/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use relix_core::{
    ConnectionConfig, ConnectionSettings, RowBatch, Secret, StatementOutcome, TableRef, TlsMode,
    TlsSettings,
};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, ExecuteOptions, SqlConnection};
use relix_driver_oracle::OracleDriver;
use relix_testing::integration::{integration_enabled, unique_name};
use std::collections::BTreeMap;
use std::env;

const DEFAULT_PORT: u16 = 51521;
const DEFAULT_USER: &str = "relix";
const DEFAULT_PASSWORD: &str = "relix_test";
const DEFAULT_SERVICE: &str = "FREEPDB1";

/// Settings for the Oracle test container (`RELIX_TEST_ORACLE_*`), or `None` unless
/// `RELIX_INTEGRATION=1`. `RELIX_TEST_ORACLE_CLIENT_LIB_DIR` points the driver at an
/// Oracle Instant Client that is not on the default library search path.
pub fn config() -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping Oracle integration test: set RELIX_INTEGRATION=1");
        return None;
    }
    let read = |suffix: &str| env::var(format!("RELIX_TEST_ORACLE_{suffix}")).ok();
    let mut properties = BTreeMap::new();
    if let Some(directory) = read("CLIENT_LIB_DIR") {
        properties.insert("client_lib_dir".to_owned(), directory);
    }
    let settings = ConnectionSettings {
        host: Some(read("HOST").unwrap_or_else(|| "127.0.0.1".to_owned())),
        port: Some(
            read("PORT")
                .and_then(|port| port.parse().ok())
                .unwrap_or(DEFAULT_PORT),
        ),
        username: Some(read("USER").unwrap_or_else(|| DEFAULT_USER.to_owned())),
        database: Some(read("DATABASE").unwrap_or_else(|| DEFAULT_SERVICE.to_owned())),
        connect_timeout_secs: 120,
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        properties,
        ..ConnectionSettings::default()
    };
    let password = read("PASSWORD").unwrap_or_else(|| DEFAULT_PASSWORD.to_owned());
    Some(ConnectionConfig::new(settings, Some(Secret::new(password))))
}

pub fn schema(config: &ConnectionConfig) -> String {
    config.settings.username.clone().unwrap().to_uppercase()
}

pub async fn connect(config: &ConnectionConfig) -> Box<dyn DatabaseConnection> {
    OracleDriver::new()
        .connect(config)
        .await
        .unwrap_or_else(|error| panic!("connect failed: {error}"))
}

pub fn sql(connection: &dyn DatabaseConnection) -> &dyn SqlConnection {
    connection
        .as_sql()
        .unwrap_or_else(|| panic!("Oracle connections are SQL connections"))
}

pub async fn run(connection: &dyn DatabaseConnection, statement: &str) -> StatementOutcome {
    sql(connection)
        .execute(statement, &ExecuteOptions::default())
        .await
        .unwrap_or_else(|error| panic!("{statement} failed: {error}"))
}

pub fn rows_of(outcome: StatementOutcome) -> RowBatch {
    match outcome {
        StatementOutcome::Rows(batch) => batch,
        other => panic!("expected rows, got {other:?}"),
    }
}

pub fn object_name(prefix: &str) -> String {
    unique_name(prefix).to_uppercase()
}

pub fn table_ref(config: &ConnectionConfig, name: &str) -> TableRef {
    TableRef::new(config.settings.database.clone(), Some(schema(config)), name)
}

pub async fn create_items(connection: &dyn DatabaseConnection) -> String {
    let name = object_name("items");
    run(
        connection,
        &format!("CREATE TABLE {name} (id NUMBER(10) PRIMARY KEY, label VARCHAR2(40) NOT NULL)"),
    )
    .await;
    run(
        connection,
        &format!(
            "INSERT INTO {name} SELECT level, CHR(96 + level) FROM dual CONNECT BY level <= 5"
        ),
    )
    .await;
    run(connection, "COMMIT").await;
    name
}
