/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{ConnectionConfig, ConnectionSettings, Secret, TlsMode, TlsSettings};
use std::env;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestEngine {
    Postgres,
    MySql,
    MariaDb,
    Redis,
}

impl TestEngine {
    fn env_prefix(self) -> &'static str {
        match self {
            Self::Postgres => "RELIX_TEST_POSTGRES",
            Self::MySql => "RELIX_TEST_MYSQL",
            Self::MariaDb => "RELIX_TEST_MARIADB",
            Self::Redis => "RELIX_TEST_REDIS",
        }
    }

    fn default_port(self) -> u16 {
        match self {
            Self::Postgres => 55432,
            Self::MySql => 53306,
            Self::MariaDb => 53307,
            Self::Redis => 56379,
        }
    }

    fn default_username(self) -> Option<&'static str> {
        match self {
            Self::Redis => None,
            _ => Some("relix"),
        }
    }

    fn default_database(self) -> &'static str {
        match self {
            Self::Redis => "0",
            _ => "relix_test",
        }
    }
}

pub fn integration_enabled() -> bool {
    env::var("RELIX_INTEGRATION").is_ok_and(|value| value == "1" || value == "true")
}

/// Connection settings for the databases defined in `docker-compose.test.yml`.
/// Returns `None` unless `RELIX_INTEGRATION=1` is set, so integration tests can skip.
pub fn integration_config(engine: TestEngine) -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping {engine:?} integration test: set RELIX_INTEGRATION=1");
        return None;
    }
    let prefix = engine.env_prefix();
    let read = |suffix: &str| env::var(format!("{prefix}_{suffix}")).ok();
    let port = read("PORT")
        .and_then(|port| port.parse().ok())
        .unwrap_or(engine.default_port());
    let settings = ConnectionSettings {
        host: Some(read("HOST").unwrap_or_else(|| "127.0.0.1".to_owned())),
        port: Some(port),
        username: read("USER").or_else(|| engine.default_username().map(str::to_owned)),
        database: Some(read("DATABASE").unwrap_or_else(|| engine.default_database().to_owned())),
        connect_timeout_secs: 10,
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        ..ConnectionSettings::default()
    };
    let password = read("PASSWORD").unwrap_or_else(|| "relix_test".to_owned());
    Some(ConnectionConfig::new(settings, Some(Secret::new(password))))
}

pub fn unique_name(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("{prefix}_{}_{}", std::process::id(), nanos % 1_000_000_000)
}
