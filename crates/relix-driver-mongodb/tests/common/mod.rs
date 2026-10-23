/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code, clippy::expect_used)]

use relix_core::{ConnectionConfig, ConnectionSettings, Secret, TlsMode, TlsSettings};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, DocumentConnection, QueryExecutor};
use relix_driver_mongodb::MongoDbDriver;
use relix_testing::integration::integration_enabled;
use std::env;

const PREFIX: &str = "RELIX_TEST_MONGODB";

fn read(suffix: &str) -> Option<String> {
    env::var(format!("{PREFIX}_{suffix}")).ok()
}

/// Settings for the `mongodb` service of `docker-compose.test.yml` (root user
/// `relix`/`relix_test` on port 57017), overridable through `RELIX_TEST_MONGODB_*`.
pub fn mongodb_config() -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping MongoDB integration test: set RELIX_INTEGRATION=1");
        return None;
    }
    let settings = ConnectionSettings {
        host: Some(read("HOST").unwrap_or_else(|| "127.0.0.1".to_owned())),
        port: Some(
            read("PORT")
                .and_then(|port| port.parse().ok())
                .unwrap_or(57017),
        ),
        username: Some(read("USER").unwrap_or_else(|| "relix".to_owned())),
        database: Some(read("DATABASE").unwrap_or_else(|| "relix_test".to_owned())),
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

pub async fn connect_with(config: &ConnectionConfig) -> Box<dyn DatabaseConnection> {
    MongoDbDriver::new().connect(config).await.expect("connect")
}

pub async fn connect() -> Option<Box<dyn DatabaseConnection>> {
    let config = mongodb_config()?;
    Some(connect_with(&config).await)
}

pub fn documents(connection: &dyn DatabaseConnection) -> &dyn DocumentConnection {
    connection.as_document().expect("document connection")
}

pub fn console(connection: &dyn DatabaseConnection) -> &dyn QueryExecutor {
    connection.as_query().expect("query executor")
}

pub fn database_name() -> String {
    read("DATABASE").unwrap_or_else(|| "relix_test".to_owned())
}
