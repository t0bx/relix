/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code)]

use relix_core::{ConnectionConfig, StatementOutcome, TableRef};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, ExecuteOptions, SqlConnection};
use relix_driver_postgres::PostgresDriver;
use relix_testing::integration::{TestEngine, integration_config, unique_name};

pub const SCHEMA: &str = "public";

pub fn config() -> Option<ConnectionConfig> {
    integration_config(TestEngine::Postgres)
}

pub async fn connect(config: &ConnectionConfig) -> Box<dyn DatabaseConnection> {
    PostgresDriver::new()
        .connect(config)
        .await
        .unwrap_or_else(|error| panic!("connect failed: {error}"))
}

pub fn sql(connection: &dyn DatabaseConnection) -> &dyn SqlConnection {
    connection
        .as_sql()
        .unwrap_or_else(|| panic!("postgres connections are SQL connections"))
}

pub async fn run(connection: &dyn DatabaseConnection, statement: &str) -> StatementOutcome {
    sql(connection)
        .execute(statement, &ExecuteOptions::default())
        .await
        .unwrap_or_else(|error| panic!("{statement} failed: {error}"))
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
