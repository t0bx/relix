/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code)]

use relix_core::{
    ConnectionConfig, ConnectionSettings, RowBatch, StatementOutcome, TlsMode, TlsSettings,
};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, ExecuteOptions, SqlConnection};
use relix_driver_trino::TrinoDriver;
use relix_testing::integration::{integration_enabled, unique_name};
use std::env;

const ENV_PREFIX: &str = "RELIX_TEST_TRINO";
pub const TPCH: &str = "tpch";
pub const MEMORY: &str = "memory";

/// Settings for the `trino` service of `docker-compose.test.yml`, overridable with
/// `RELIX_TEST_TRINO_{HOST,PORT,USER,CATALOG,SCHEMA}`. The default catalog is the
/// built-in `tpch` connector with its `tiny` schema. Returns `None` unless
/// `RELIX_INTEGRATION=1` is set.
pub fn config() -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping Trino integration test: set RELIX_INTEGRATION=1");
        return None;
    }
    let read = |suffix: &str| env::var(format!("{ENV_PREFIX}_{suffix}")).ok();
    let mut settings = ConnectionSettings {
        host: Some(read("HOST").unwrap_or_else(|| "127.0.0.1".to_owned())),
        port: Some(
            read("PORT")
                .and_then(|port| port.parse().ok())
                .unwrap_or(58080),
        ),
        username: Some(read("USER").unwrap_or_else(|| "relix".to_owned())),
        database: Some(read("CATALOG").unwrap_or_else(|| TPCH.to_owned())),
        connect_timeout_secs: 30,
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        ..ConnectionSettings::default()
    };
    settings.properties.insert(
        "schema".into(),
        read("SCHEMA").unwrap_or_else(|| "tiny".to_owned()),
    );
    Some(ConnectionConfig::new(settings, None))
}

pub async fn connect(config: &ConnectionConfig) -> Box<dyn DatabaseConnection> {
    TrinoDriver::new()
        .connect(config)
        .await
        .unwrap_or_else(|error| panic!("connect failed: {error}"))
}

pub fn sql(connection: &dyn DatabaseConnection) -> &dyn SqlConnection {
    connection
        .as_sql()
        .unwrap_or_else(|| panic!("trino connections are SQL connections"))
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
        other => panic!("{statement} returned {other:?} instead of rows"),
    }
}

pub fn table_name(prefix: &str) -> String {
    unique_name(prefix)
}
