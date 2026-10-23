/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

use relix_core::{ConnectionConfig, ConnectionSettings, Secret};
use relix_driver_api::{DatabaseDriver, ErrorKind};
use relix_driver_oracle::OracleDriver;

#[tokio::test]
async fn missing_instant_client_is_a_configuration_error() {
    let mut settings = ConnectionSettings {
        host: Some("127.0.0.1".into()),
        port: Some(1),
        username: Some("relix".into()),
        database: Some("FREEPDB1".into()),
        ..ConnectionSettings::default()
    };
    settings.properties.insert(
        "client_lib_dir".into(),
        "/nonexistent/relix/instantclient".into(),
    );
    let config = ConnectionConfig::new(settings, Some(Secret::new("secret-password")));
    let error = OracleDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Configuration, "{error}");
    assert!(error.message.contains("Instant Client"), "{error}");
    assert!(!error.to_string().contains("secret-password"));
}
