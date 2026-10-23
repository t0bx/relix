/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use relix_core::{
    ConnectionConfig, ConnectionSettings, Secret, StatementOutcome, TableRef, TlsMode, TlsSettings,
};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, ExecuteOptions, SqlConnection};
use relix_driver_cassandra::{CassandraDriver, ScyllaDbDriver};
use relix_testing::integration::{integration_enabled, unique_name};
use std::env;

/// The CQL server under test: `RELIX_TEST_CQL_ENGINE=scylladb` (default, port 59042) or
/// `cassandra` (port 59043). Host, port, user and password can be overridden with
/// `RELIX_TEST_<ENGINE>_HOST/_PORT/_USER/_PASSWORD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    ScyllaDb,
    Cassandra,
}

impl Engine {
    pub fn current() -> Self {
        match env::var("RELIX_TEST_CQL_ENGINE").as_deref() {
            Ok("cassandra") => Self::Cassandra,
            _ => Self::ScyllaDb,
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            Self::ScyllaDb => "RELIX_TEST_SCYLLADB",
            Self::Cassandra => "RELIX_TEST_CASSANDRA",
        }
    }

    fn default_port(self) -> u16 {
        match self {
            Self::ScyllaDb => 59042,
            Self::Cassandra => 59043,
        }
    }

    pub fn product(self) -> &'static str {
        match self {
            Self::ScyllaDb => "ScyllaDB",
            Self::Cassandra => "Apache Cassandra",
        }
    }

    pub fn driver(self) -> Box<dyn DatabaseDriver> {
        match self {
            Self::ScyllaDb => Box::new(ScyllaDbDriver::new()),
            Self::Cassandra => Box::new(CassandraDriver::new()),
        }
    }
}

pub fn config() -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping CQL integration test: set RELIX_INTEGRATION=1");
        return None;
    }
    let engine = Engine::current();
    let read = |suffix: &str| env::var(format!("{}_{suffix}", engine.prefix())).ok();
    let settings = ConnectionSettings {
        host: Some(read("HOST").unwrap_or_else(|| "127.0.0.1".to_owned())),
        port: Some(
            read("PORT")
                .and_then(|port| port.parse().ok())
                .unwrap_or(engine.default_port()),
        ),
        username: Some(read("USER").unwrap_or_else(|| "cassandra".to_owned())),
        connect_timeout_secs: 20,
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        ..ConnectionSettings::default()
    };
    let password = read("PASSWORD").unwrap_or_else(|| "cassandra".to_owned());
    Some(ConnectionConfig::new(settings, Some(Secret::new(password))))
}

pub async fn connect(config: &ConnectionConfig) -> Box<dyn DatabaseConnection> {
    Engine::current()
        .driver()
        .connect(config)
        .await
        .unwrap_or_else(|error| panic!("connect failed: {error}"))
}

pub fn sql(connection: &dyn DatabaseConnection) -> &dyn SqlConnection {
    connection
        .as_sql()
        .unwrap_or_else(|| panic!("CQL connections expose the tabular interface"))
}

pub async fn run(connection: &dyn DatabaseConnection, statement: &str) -> StatementOutcome {
    sql(connection)
        .execute(statement, &ExecuteOptions::default())
        .await
        .unwrap_or_else(|error| panic!("{statement} failed: {error}"))
}

/// Creates a uniquely named keyspace with replication factor 1. ScyllaDB keyspaces
/// default to tablets on recent versions; the tests use vnodes so that lightweight
/// transactions, materialized views and secondary indexes behave the same everywhere.
pub async fn create_keyspace(connection: &dyn DatabaseConnection) -> String {
    let keyspace = unique_name("relix_ks");
    let base = format!(
        "CREATE KEYSPACE {keyspace} WITH replication = \
         {{'class': 'SimpleStrategy', 'replication_factor': 1}}"
    );
    let options = ExecuteOptions::default();
    let with_vnodes = format!("{base} AND tablets = {{'enabled': false}}");
    if sql(connection)
        .execute(&with_vnodes, &options)
        .await
        .is_err()
    {
        run(connection, &base).await;
    }
    keyspace
}

pub async fn drop_keyspace(connection: &dyn DatabaseConnection, keyspace: &str) {
    run(connection, &format!("DROP KEYSPACE IF EXISTS {keyspace}")).await;
}

pub fn table_ref(keyspace: &str, name: &str) -> TableRef {
    TableRef::new(Some(keyspace.to_owned()), None, name)
}

pub fn rows(outcome: StatementOutcome) -> relix_core::RowBatch {
    match outcome {
        StatementOutcome::Rows(batch) => batch,
        other => panic!("expected rows, got {other:?}"),
    }
}
