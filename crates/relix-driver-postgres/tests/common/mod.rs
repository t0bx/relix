/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code)]

use relix_core::{
    ConnectionConfig, ConnectionSettings, Secret, StatementOutcome, TableRef, TlsMode, TlsSettings,
};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, ExecuteOptions, SqlConnection};
use relix_driver_postgres::{
    CockroachDbDriver, PostgresDriver, TimescaleDbDriver, YugabyteDbDriver,
};
use relix_testing::integration::{
    TestEngine, integration_config, integration_enabled, unique_name,
};
use std::env;

pub const SCHEMA: &str = "public";

#[allow(unused_macros)]
macro_rules! for_each_engine {
    ($($test:ident),* $(,)?) => {
        for_each_engine!(@engine postgres, Postgres, $($test),*);
        for_each_engine!(@engine cockroachdb, CockroachDb, $($test),*);
        for_each_engine!(@engine yugabytedb, YugabyteDb, $($test),*);
        for_each_engine!(@engine timescaledb, TimescaleDb, $($test),*);
    };
    (@engine $module:ident, $engine:ident, $($test:ident),*) => {
        mod $module {
            $(
                #[tokio::test(flavor = "multi_thread")]
                async fn $test() {
                    super::$test(super::common::Engine::$engine).await;
                }
            )*
        }
    };
}

#[allow(unused_imports)]
pub(crate) use for_each_engine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Postgres,
    CockroachDb,
    YugabyteDb,
    TimescaleDb,
}

struct EngineDefaults {
    env_prefix: &'static str,
    port: u16,
    user: &'static str,
    password: Option<&'static str>,
    database: &'static str,
}

impl Engine {
    pub fn driver(self) -> Box<dyn DatabaseDriver> {
        match self {
            Self::Postgres => Box::new(PostgresDriver::new()),
            Self::CockroachDb => Box::new(CockroachDbDriver::new()),
            Self::YugabyteDb => Box::new(YugabyteDbDriver::new()),
            Self::TimescaleDb => Box::new(TimescaleDbDriver::new()),
        }
    }

    pub fn product(self) -> &'static str {
        match self {
            Self::Postgres => "PostgreSQL",
            Self::CockroachDb => "CockroachDB",
            Self::YugabyteDb => "YugabyteDB",
            Self::TimescaleDb => "TimescaleDB",
        }
    }

    /// The single-node CockroachDB test cluster runs in insecure mode, which skips
    /// password checks and offers no TLS.
    pub fn checks_passwords(self) -> bool {
        self != Self::CockroachDb
    }

    /// How DDL names an object: CockroachDB returns its native `SHOW CREATE` output,
    /// the other engines a reconstruction with quoted identifiers.
    pub fn ddl_name(self, schema: &str, name: &str) -> String {
        match self {
            Self::CockroachDb => format!("{schema}.{name}"),
            Self::Postgres | Self::YugabyteDb | Self::TimescaleDb => {
                format!("\"{schema}\".\"{name}\"")
            }
        }
    }

    /// YugabyteDB raises unique violations from its storage layer without the
    /// `Key (...)=(...) already exists.` detail PostgreSQL adds.
    pub fn details_unique_violations(self) -> bool {
        self != Self::YugabyteDb
    }

    pub fn config(self) -> Option<ConnectionConfig> {
        let Some(defaults) = self.defaults() else {
            return integration_config(TestEngine::Postgres);
        };
        if !integration_enabled() {
            eprintln!("skipping {self:?} integration test: set RELIX_INTEGRATION=1");
            return None;
        }
        let read = |suffix: &str| env::var(format!("{}_{suffix}", defaults.env_prefix)).ok();
        let settings = ConnectionSettings {
            host: Some(read("HOST").unwrap_or_else(|| "127.0.0.1".to_owned())),
            port: Some(
                read("PORT")
                    .and_then(|port| port.parse().ok())
                    .unwrap_or(defaults.port),
            ),
            username: Some(read("USER").unwrap_or_else(|| defaults.user.to_owned())),
            database: Some(read("DATABASE").unwrap_or_else(|| defaults.database.to_owned())),
            connect_timeout_secs: 10,
            tls: TlsSettings {
                mode: TlsMode::Disable,
                ca_certificate: None,
            },
            ..ConnectionSettings::default()
        };
        let password = read("PASSWORD").or_else(|| defaults.password.map(str::to_owned));
        Some(ConnectionConfig::new(settings, password.map(Secret::new)))
    }

    fn defaults(self) -> Option<EngineDefaults> {
        match self {
            Self::Postgres => None,
            Self::CockroachDb => Some(EngineDefaults {
                env_prefix: "RELIX_TEST_COCKROACHDB",
                port: 56257,
                user: "root",
                password: None,
                database: "defaultdb",
            }),
            Self::YugabyteDb => Some(EngineDefaults {
                env_prefix: "RELIX_TEST_YUGABYTEDB",
                port: 55433,
                user: "yugabyte",
                password: Some("yugabyte"),
                database: "yugabyte",
            }),
            Self::TimescaleDb => Some(EngineDefaults {
                env_prefix: "RELIX_TEST_TIMESCALEDB",
                port: 55434,
                user: "relix",
                password: Some("relix_test"),
                database: "relix_test",
            }),
        }
    }
}

pub async fn connect(engine: Engine, config: &ConnectionConfig) -> Box<dyn DatabaseConnection> {
    engine
        .driver()
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
