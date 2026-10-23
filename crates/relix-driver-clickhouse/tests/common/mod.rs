/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code)]

use relix_core::{
    ConnectionConfig, ConnectionSettings, RowBatch, Secret, StatementOutcome, TlsMode, TlsSettings,
};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, ExecuteOptions, SqlConnection};
use relix_driver_clickhouse::ClickHouseDriver;
use relix_testing::integration::{integration_enabled, unique_name};
use std::env;

const ENV_PREFIX: &str = "RELIX_TEST_CLICKHOUSE";

/// Settings for the `clickhouse` service of `docker-compose.test.yml`, overridable with
/// `RELIX_TEST_CLICKHOUSE_{HOST,PORT,USER,PASSWORD,DATABASE}`. Returns `None` unless
/// `RELIX_INTEGRATION=1` is set.
pub fn config() -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping ClickHouse integration test: set RELIX_INTEGRATION=1");
        return None;
    }
    let read = |suffix: &str| env::var(format!("{ENV_PREFIX}_{suffix}")).ok();
    let settings = ConnectionSettings {
        host: Some(read("HOST").unwrap_or_else(|| "127.0.0.1".to_owned())),
        port: Some(
            read("PORT")
                .and_then(|port| port.parse().ok())
                .unwrap_or(58123),
        ),
        username: Some(read("USER").unwrap_or_else(|| "relix".to_owned())),
        database: Some(read("DATABASE").unwrap_or_else(|| "relix_test".to_owned())),
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        ..ConnectionSettings::default()
    };
    let password = read("PASSWORD").unwrap_or_else(|| "relix_test".to_owned());
    Some(ConnectionConfig::new(settings, Some(Secret::new(password))))
}

pub fn database(config: &ConnectionConfig) -> String {
    config.settings.database.clone().unwrap_or_default()
}

pub async fn connect(config: &ConnectionConfig) -> Box<dyn DatabaseConnection> {
    ClickHouseDriver::new()
        .connect(config)
        .await
        .unwrap_or_else(|error| panic!("connect failed: {error}"))
}

pub fn sql(connection: &dyn DatabaseConnection) -> &dyn SqlConnection {
    connection
        .as_sql()
        .unwrap_or_else(|| panic!("clickhouse connections are SQL connections"))
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
