/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code)]

use relix_core::{ConnectionConfig, ConnectionSettings, RowBatch, StatementOutcome};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, ExecuteOptions, SqlConnection};
use relix_driver_duckdb::DuckDbDriver;
use std::path::PathBuf;
use tempfile::TempDir;

pub const DATABASE: &str = "relix_test";
pub const SCHEMA: &str = "main";

pub struct TestDatabase {
    pub directory: TempDir,
    pub config: ConnectionConfig,
}

impl TestDatabase {
    pub fn path(&self) -> PathBuf {
        self.directory.path().join(format!("{DATABASE}.duckdb"))
    }
}

pub fn file_database() -> TestDatabase {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
    let settings = ConnectionSettings {
        file_path: Some(directory.path().join(format!("{DATABASE}.duckdb"))),
        create_if_missing: true,
        ..ConnectionSettings::default()
    };
    TestDatabase {
        directory,
        config: ConnectionConfig::new(settings, None),
    }
}

pub async fn connect(config: &ConnectionConfig) -> Box<dyn DatabaseConnection> {
    DuckDbDriver::new()
        .connect(config)
        .await
        .unwrap_or_else(|error| panic!("connect failed: {error}"))
}

pub fn sql(connection: &dyn DatabaseConnection) -> &dyn SqlConnection {
    connection
        .as_sql()
        .unwrap_or_else(|| panic!("duckdb connections are SQL connections"))
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
