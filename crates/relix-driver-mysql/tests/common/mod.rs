/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code)]

use relix_core::{ConnectionConfig, ConnectionSettings, Secret, TlsMode, TlsSettings};
use relix_driver_api::DatabaseDriver;
use relix_driver_mysql::{MariaDbDriver, MySqlDriver, TiDbDriver};
use relix_testing::integration::{TestEngine, integration_config, integration_enabled};
use std::env;

const TIDB_ENV_PREFIX: &str = "RELIX_TEST_TIDB";
const TIDB_PORT: u16 = 54000;
const TIDB_USER: &str = "root";
const TIDB_DATABASE: &str = "test";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    MySql,
    MariaDb,
    TiDb,
}

impl Engine {
    pub fn driver(self) -> Box<dyn DatabaseDriver> {
        match self {
            Self::MySql => Box::new(MySqlDriver::new()),
            Self::MariaDb => Box::new(MariaDbDriver::new()),
            Self::TiDb => Box::new(TiDbDriver::new()),
        }
    }

    pub fn product(self) -> &'static str {
        match self {
            Self::MySql => "MySQL",
            Self::MariaDb => "MariaDB",
            Self::TiDb => "TiDB",
        }
    }

    pub fn supports_routines(self) -> bool {
        self != Self::TiDb
    }

    pub fn supports_sequences(self) -> bool {
        self != Self::MySql
    }

    pub fn config(self) -> Option<ConnectionConfig> {
        match self {
            Self::MySql => integration_config(TestEngine::MySql),
            Self::MariaDb => integration_config(TestEngine::MariaDb),
            Self::TiDb => tidb_config(),
        }
    }
}

/// The TiDB test server runs with its default passwordless `root` account and the
/// built-in `test` database.
fn tidb_config() -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping TiDB integration test: set RELIX_INTEGRATION=1");
        return None;
    }
    let read = |suffix: &str| env::var(format!("{TIDB_ENV_PREFIX}_{suffix}")).ok();
    let settings = ConnectionSettings {
        host: Some(read("HOST").unwrap_or_else(|| "127.0.0.1".to_owned())),
        port: Some(
            read("PORT")
                .and_then(|port| port.parse().ok())
                .unwrap_or(TIDB_PORT),
        ),
        username: Some(read("USER").unwrap_or_else(|| TIDB_USER.to_owned())),
        database: Some(read("DATABASE").unwrap_or_else(|| TIDB_DATABASE.to_owned())),
        connect_timeout_secs: 10,
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        ..ConnectionSettings::default()
    };
    Some(ConnectionConfig::new(
        settings,
        read("PASSWORD").map(Secret::new),
    ))
}
