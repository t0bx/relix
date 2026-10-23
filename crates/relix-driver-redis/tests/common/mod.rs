/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code)]

use relix_core::{ConnectionConfig, ConnectionSettings, Secret, TlsMode, TlsSettings};
use relix_driver_api::DatabaseDriver;
use relix_driver_redis::{RedisDriver, ValkeyDriver};
use relix_testing::integration::{TestEngine, integration_config, integration_enabled};
use std::env;

const VALKEY_ENV_PREFIX: &str = "RELIX_TEST_VALKEY";
const VALKEY_PORT: u16 = 56380;
const VALKEY_PASSWORD: &str = "relix_test";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Redis,
    Valkey,
}

impl Engine {
    pub fn driver(self) -> Box<dyn DatabaseDriver> {
        match self {
            Self::Redis => Box::new(RedisDriver::new()),
            Self::Valkey => Box::new(ValkeyDriver::new()),
        }
    }

    pub fn product(self) -> &'static str {
        match self {
            Self::Redis => "Redis",
            Self::Valkey => "Valkey",
        }
    }

    pub fn version_key(self) -> &'static str {
        match self {
            Self::Redis => "server.redis_version",
            Self::Valkey => "server.valkey_version",
        }
    }

    pub fn config(self) -> Option<ConnectionConfig> {
        match self {
            Self::Redis => integration_config(TestEngine::Redis),
            Self::Valkey => valkey_config(),
        }
    }
}

fn valkey_config() -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping Valkey integration test: set RELIX_INTEGRATION=1");
        return None;
    }
    let read = |suffix: &str| env::var(format!("{VALKEY_ENV_PREFIX}_{suffix}")).ok();
    let settings = ConnectionSettings {
        host: Some(read("HOST").unwrap_or_else(|| "127.0.0.1".to_owned())),
        port: Some(
            read("PORT")
                .and_then(|port| port.parse().ok())
                .unwrap_or(VALKEY_PORT),
        ),
        username: read("USER"),
        database: Some(read("DATABASE").unwrap_or_else(|| "0".to_owned())),
        connect_timeout_secs: 10,
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        ..ConnectionSettings::default()
    };
    let password = read("PASSWORD").unwrap_or_else(|| VALKEY_PASSWORD.to_owned());
    Some(ConnectionConfig::new(settings, Some(Secret::new(password))))
}
