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
use relix_driver_mssql::MsSqlDriver;
use relix_testing::integration::{integration_enabled, unique_name};
use std::env;
use tokio::sync::OnceCell;

pub const SCHEMA: &str = "dbo";
const DEFAULT_PORT: u16 = 51433;
const DEFAULT_USER: &str = "sa";
const DEFAULT_PASSWORD: &str = "Relix_test1!";
const DEFAULT_DATABASE: &str = "relix_test";

static DATABASE_READY: OnceCell<()> = OnceCell::const_new();

/// Settings for the SQL Server test container (`RELIX_TEST_MSSQL_*`), or `None` unless
/// `RELIX_INTEGRATION=1`. The test database is created on first use, because the
/// container image only ships the system databases.
pub async fn config() -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping SQL Server integration test: set RELIX_INTEGRATION=1");
        return None;
    }
    let config = base_config();
    DATABASE_READY
        .get_or_init(|| create_database(config.clone()))
        .await;
    Some(config)
}

fn base_config() -> ConnectionConfig {
    let read = |suffix: &str| env::var(format!("RELIX_TEST_MSSQL_{suffix}")).ok();
    let settings = ConnectionSettings {
        host: Some(read("HOST").unwrap_or_else(|| "127.0.0.1".to_owned())),
        port: Some(
            read("PORT")
                .and_then(|port| port.parse().ok())
                .unwrap_or(DEFAULT_PORT),
        ),
        username: Some(read("USER").unwrap_or_else(|| DEFAULT_USER.to_owned())),
        database: Some(read("DATABASE").unwrap_or_else(|| DEFAULT_DATABASE.to_owned())),
        connect_timeout_secs: 20,
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        ..ConnectionSettings::default()
    };
    let password = read("PASSWORD").unwrap_or_else(|| DEFAULT_PASSWORD.to_owned());
    ConnectionConfig::new(settings, Some(Secret::new(password)))
}

async fn create_database(config: ConnectionConfig) {
    let database = config.settings.database.clone().unwrap();
    let mut master = config;
    master.settings.database = Some("master".into());
    let connection = connect(&master).await;
    run(
        connection.as_ref(),
        &format!("IF DB_ID(N'{database}') IS NULL CREATE DATABASE [{database}]"),
    )
    .await;
    connection.close().await.unwrap();
}

pub async fn connect(config: &ConnectionConfig) -> Box<dyn DatabaseConnection> {
    MsSqlDriver::new()
        .connect(config)
        .await
        .unwrap_or_else(|error| panic!("connect failed: {error}"))
}

pub fn sql(connection: &dyn DatabaseConnection) -> &dyn SqlConnection {
    connection
        .as_sql()
        .unwrap_or_else(|| panic!("SQL Server connections are SQL connections"))
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

pub fn table_name(prefix: &str) -> String {
    unique_name(prefix)
}

pub fn table_ref(config: &ConnectionConfig, name: &str) -> TableRef {
    TableRef::new(
        config.settings.database.clone(),
        Some(SCHEMA.to_owned()),
        name,
    )
}

pub async fn create_items(connection: &dyn DatabaseConnection) -> String {
    let name = table_name("items");
    run(
        connection,
        &format!("CREATE TABLE [{name}] (id INT PRIMARY KEY, label NVARCHAR(40) NOT NULL)"),
    )
    .await;
    run(
        connection,
        &format!(
            "INSERT INTO [{name}] VALUES (1, N'a'), (2, N'b'), (3, N'c'), (4, N'd'), (5, N'e')"
        ),
    )
    .await;
    name
}
