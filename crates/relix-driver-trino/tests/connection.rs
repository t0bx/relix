/*
 * @author t0bx
 * @since 23.10.2026
 */

mod common;

use relix_core::{Secret, TlsMode};
use relix_driver_api::{DatabaseDriver, ErrorKind};
use relix_driver_trino::TrinoDriver;

#[tokio::test]
async fn connects_and_reports_the_coordinator() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let info = connection.server_info();
    assert_eq!(info.product, "Trino");
    assert!(
        info.version
            .chars()
            .all(|ch| ch.is_ascii_digit() || ch == '.')
    );
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
async fn passwords_are_never_sent_over_plain_http() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.password = Some(Secret::new("secret"));
    let error = TrinoDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Configuration);

    config.settings.tls.mode = TlsMode::Prefer;
    config.settings.connect_timeout_secs = 5;
    let upgraded = TrinoDriver::new().connect(&config).await.err().unwrap();
    assert!(
        matches!(
            upgraded.kind,
            ErrorKind::Tls | ErrorKind::Connection | ErrorKind::Protocol | ErrorKind::Timeout
        ),
        "{upgraded:?}"
    );
    assert!(!upgraded.to_string().contains("secret"));
}

#[tokio::test]
async fn unreachable_coordinators_are_connection_errors() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.settings.port = Some(1);
    let error = TrinoDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Connection);
}
