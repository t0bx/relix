/*
 * @author t0bx
 * @since 24.10.2026
 */

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use relix_core::{
    ConnectionConfig, ConnectionSettings, RowBatch, Secret, StatementOutcome, TlsMode, TlsSettings,
};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, ExecuteOptions, SqlConnection};
use relix_driver_http::{EndpointOptions, HttpAuth, HttpEndpoint, Method};
use relix_driver_influxdb::InfluxDbDriver;
use relix_testing::integration::{integration_enabled, unique_name};
use std::env;

/// InfluxDB 3 Core on port 58181 (`RELIX_TEST_INFLUXDB_HOST/_PORT`). Servers started
/// with authentication need `RELIX_TEST_INFLUXDB_TOKEN`; without it no token is sent.
pub fn config() -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping InfluxDB integration test: set RELIX_INTEGRATION=1");
        return None;
    }
    let read = |suffix: &str| env::var(format!("RELIX_TEST_INFLUXDB_{suffix}")).ok();
    let settings = ConnectionSettings {
        host: Some(read("HOST").unwrap_or_else(|| "127.0.0.1".to_owned())),
        port: Some(
            read("PORT")
                .and_then(|port| port.parse().ok())
                .unwrap_or(58181),
        ),
        connect_timeout_secs: 10,
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        ..ConnectionSettings::default()
    };
    Some(ConnectionConfig::new(
        settings,
        read("TOKEN").map(Secret::new),
    ))
}

pub fn has_token() -> bool {
    env::var("RELIX_TEST_INFLUXDB_TOKEN").is_ok()
}

pub async fn connect(config: &ConnectionConfig) -> Box<dyn DatabaseConnection> {
    InfluxDbDriver::new()
        .connect(config)
        .await
        .unwrap_or_else(|error| panic!("connect failed: {error}"))
}

pub fn sql(connection: &dyn DatabaseConnection) -> &dyn SqlConnection {
    connection
        .as_sql()
        .unwrap_or_else(|| panic!("InfluxDB connections expose the tabular interface"))
}

/// Creates a uniquely named database and writes the given line protocol into it with
/// nanosecond precision. The driver itself does not write points.
pub async fn database_with(config: &ConnectionConfig, lines: &str) -> String {
    let database = unique_name("relix_db");
    let endpoint = admin_endpoint(config).await;
    endpoint
        .send(
            endpoint
                .request(Method::POST, "/api/v3/configure/database")
                .json(&serde_json::json!({ "db": database })),
        )
        .await
        .unwrap();
    endpoint
        .send(
            endpoint
                .request(
                    Method::POST,
                    &format!("/api/v3/write_lp?db={database}&precision=nanosecond"),
                )
                .body(lines.to_owned()),
        )
        .await
        .unwrap();
    database
}

/// Drops a test database again; InfluxDB 3 Core allows only five databases.
pub async fn drop_database(config: &ConnectionConfig, database: &str) {
    let endpoint = admin_endpoint(config).await;
    endpoint
        .send(endpoint.request(
            Method::DELETE,
            &format!("/api/v3/configure/database?db={database}"),
        ))
        .await
        .unwrap();
}

async fn admin_endpoint(config: &ConnectionConfig) -> HttpEndpoint {
    let auth = config
        .password
        .clone()
        .map_or(HttpAuth::None, HttpAuth::Bearer);
    HttpEndpoint::connect(
        config,
        EndpointOptions {
            default_port: 8181,
            probe_path: "/ping".into(),
            auth,
            request_timeout: None,
        },
    )
    .await
    .unwrap()
}

pub fn scoped(config: &ConnectionConfig, database: &str) -> ConnectionConfig {
    let mut scoped = config.clone();
    scoped.settings.database = Some(database.to_owned());
    scoped
}

pub async fn rows(connection: &dyn DatabaseConnection, statement: &str) -> RowBatch {
    match sql(connection)
        .execute(statement, &ExecuteOptions::default())
        .await
        .unwrap_or_else(|error| panic!("{statement} failed: {error}"))
    {
        StatementOutcome::Rows(batch) => batch,
        other => panic!("expected rows, got {other:?}"),
    }
}

/// 25 points of `cpu` one second apart starting at 2023-11-14T22:13:20Z, alternating
/// between two hosts, plus a `mem` point with a string field.
pub fn sample_points() -> String {
    let mut lines: Vec<String> = (0..25)
        .map(|index| {
            format!(
                "cpu,host=host{},region=eu usage={}.5,count={index}i {}",
                index % 2,
                index,
                1_700_000_000_000_000_000_i64 + index * 1_000_000_000
            )
        })
        .collect();
    lines.push("mem,host=host0 free=12i,note=\"low\" 1700000000000000000".to_owned());
    lines.join("\n")
}
