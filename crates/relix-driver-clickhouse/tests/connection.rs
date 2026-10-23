/*
 * @author t0bx
 * @since 23.10.2026
 */

mod common;

use relix_core::{Secret, Value};
use relix_driver_api::{DatabaseDriver, ErrorKind};
use relix_driver_clickhouse::ClickHouseDriver;

#[tokio::test]
async fn connects_and_reports_the_server() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let info = connection.server_info();
    assert_eq!(info.product, "ClickHouse");
    assert!(info.version.starts_with(|ch: char| ch.is_ascii_digit()));
    connection.ping().await.unwrap();
    let databases = common::sql(connection.as_ref()).databases().await.unwrap();
    assert!(
        databases
            .iter()
            .any(|database| database.name == common::database(&config) && database.is_default)
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn wrong_password_is_an_authentication_error() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.password = Some(Secret::new("definitely wrong"));
    let error = ClickHouseDriver::new()
        .connect(&config)
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication);
    assert!(!error.to_string().contains("definitely wrong"));
}

#[tokio::test]
async fn unreachable_servers_are_connection_errors() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.settings.port = Some(1);
    let error = ClickHouseDriver::new()
        .connect(&config)
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Connection);
}

#[tokio::test]
async fn unknown_databases_are_rejected_at_connect() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.settings.database = Some("relix_missing_database".into());
    let error = ClickHouseDriver::new()
        .connect(&config)
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Query);
    assert!(
        error.message.contains("UNKNOWN_DATABASE"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn basic_authentication_is_supported() {
    let Some(mut config) = common::config() else {
        return;
    };
    config
        .settings
        .properties
        .insert("auth_method".into(), "basic".into());
    let connection = common::connect(&config).await;
    let batch = common::rows(connection.as_ref(), "SELECT currentUser()").await;
    assert_eq!(
        batch.rows,
        vec![vec![Value::Text(config.settings.username.clone().unwrap())]]
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn read_only_users_can_query() {
    let Some(config) = common::config() else {
        return;
    };
    let admin = common::connect(&config).await;
    let user = common::table_name("relix_ro");
    common::run(
        admin.as_ref(),
        &format!("CREATE USER {user} IDENTIFIED WITH sha256_password BY 'ro_secret' SETTINGS readonly = 1"),
    )
    .await;
    common::run(admin.as_ref(), &format!("GRANT SELECT ON *.* TO {user}")).await;

    let mut reader_config = config.clone();
    reader_config.settings.username = Some(user.clone());
    reader_config.password = Some(Secret::new("ro_secret"));
    let reader = common::connect(&reader_config).await;
    let batch = common::rows(reader.as_ref(), "SELECT nan AS n, toDecimal64(2.5, 3) AS d").await;
    assert_eq!(batch.rows[0][1], Value::Text("2.5".into()));
    let error = common::sql(reader.as_ref())
        .execute(
            "CREATE TABLE should_fail (a UInt8) ENGINE = Memory",
            &Default::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    reader.close().await.unwrap();

    common::run(admin.as_ref(), &format!("DROP USER {user}")).await;
    admin.close().await.unwrap();
}
