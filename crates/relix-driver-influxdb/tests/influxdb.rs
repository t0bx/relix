/*
 * @author t0bx
 * @since 24.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{ChangeSet, ObjectKind, Secret, TableQuery, TableRef, TlsMode, Value};
use relix_driver_api::{DatabaseDriver, ErrorKind, ExecuteOptions};
use relix_driver_influxdb::InfluxDbDriver;

#[tokio::test]
async fn connects_and_reports_the_server() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let info = connection.server_info();
    assert!(info.product.starts_with("InfluxDB 3"), "{}", info.product);
    assert!(info.version.starts_with('3'), "{}", info.version);
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
    assert!(connection.ping().await.is_err());
}

#[tokio::test]
async fn tokens_are_checked_when_authentication_is_enabled() {
    let Some(mut config) = common::config() else {
        return;
    };
    if !common::has_token() {
        eprintln!("server runs without authentication; token checks skipped");
        return;
    }
    config.password = Some(Secret::new("apiv3_definitely-wrong"));
    let error = InfluxDbDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication, "{error}");
    assert!(!error.to_string().contains("definitely-wrong"));
    config.password = None;
    let error = InfluxDbDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication, "{error}");
}

#[tokio::test]
async fn unreachable_servers_and_tls_mismatches_fail_clearly() {
    let Some(mut config) = common::config() else {
        return;
    };
    config.settings.tls.mode = TlsMode::Require;
    let error = InfluxDbDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Tls, "{error}");
    config.settings.tls.mode = TlsMode::Disable;
    config.settings.port = Some(1);
    let error = InfluxDbDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Connection, "{error}");
}

#[tokio::test]
async fn sql_and_influxql_queries_stream_up_to_the_row_limit() {
    let Some(config) = common::config() else {
        return;
    };
    let database = common::database_with(&config, &common::sample_points()).await;
    let connection = common::connect(&common::scoped(&config, &database)).await;
    let sql = common::sql(connection.as_ref());
    let limited = match sql
        .execute(
            "SELECT time, host, usage FROM cpu ORDER BY time;",
            &ExecuteOptions { row_limit: 10 },
        )
        .await
        .unwrap()
    {
        relix_core::StatementOutcome::Rows(batch) => batch,
        other => panic!("{other:?}"),
    };
    let names: Vec<&str> = limited
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(names, ["time", "host", "usage"]);
    assert_eq!(limited.rows.len(), 10);
    assert!(limited.has_more);
    assert_eq!(
        limited.rows[0],
        vec![
            Value::Text("2023-11-14T22:13:20Z".into()),
            Value::Text("host0".into()),
            Value::Float(0.5)
        ]
    );
    let all = common::rows(connection.as_ref(), "SELECT count(*) AS n FROM cpu").await;
    assert_eq!(all.rows, vec![vec![Value::Int(25)]]);
    let empty = common::rows(connection.as_ref(), "SELECT * FROM cpu WHERE host = 'none'").await;
    assert!(empty.rows.is_empty() && !empty.has_more);
    let error = sql
        .execute("SELEC 1", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query, "{error}");
    assert_eq!(error.position, Some(0));

    let mut influxql = common::scoped(&config, &database);
    influxql
        .settings
        .properties
        .insert("query_language".into(), "influxql".into());
    let influx = common::connect(&influxql).await;
    let measurements = common::rows(influx.as_ref(), "SHOW MEASUREMENTS").await;
    let name = measurements.column_index("name").unwrap();
    let listed: Vec<&Value> = measurements.rows.iter().map(|row| &row[name]).collect();
    assert_eq!(
        listed,
        [&Value::Text("cpu".into()), &Value::Text("mem".into())]
    );
    let selected = common::rows(
        influx.as_ref(),
        "SELECT usage FROM cpu WHERE host = 'host1'",
    )
    .await;
    assert_eq!(selected.rows.len(), 12);
    influx.close().await.unwrap();
    connection.close().await.unwrap();
    common::drop_database(&config, &database).await;
}

#[tokio::test]
async fn browses_databases_schemas_measurements_and_columns() {
    let Some(config) = common::config() else {
        return;
    };
    let database = common::database_with(&config, &common::sample_points()).await;
    let connection = common::connect(&common::scoped(&config, &database)).await;
    let sql = common::sql(connection.as_ref());
    let databases = sql.databases().await.unwrap();
    let default = databases.iter().find(|item| item.is_default).unwrap();
    assert_eq!(default.name, database);
    let schemas = sql.schemas(&database).await.unwrap();
    assert_eq!(&schemas[..3], ["iox", "system", "information_schema"]);
    let measurements = sql.objects(&database, Some("iox")).await.unwrap();
    let names: Vec<&str> = measurements
        .iter()
        .map(|object| object.object.name.as_str())
        .collect();
    assert!(
        names.contains(&"cpu") && names.contains(&"mem"),
        "{names:?}"
    );
    assert_eq!(measurements[0].comment.as_deref(), Some("measurement"));
    let system = sql.objects(&database, Some("system")).await.unwrap();
    assert!(system.iter().any(|object| object.object.name == "queries"));
    let views = sql
        .objects(&database, Some("information_schema"))
        .await
        .unwrap();
    assert!(views.iter().all(|object| object.kind == ObjectKind::View));

    let cpu = TableRef::new(Some(database.clone()), Some("iox".into()), "cpu");
    let details = sql.table_details(&cpu).await.unwrap();
    let columns: Vec<(&str, Option<&str>)> = details
        .columns
        .iter()
        .map(|column| (column.name.as_str(), column.comment.as_deref()))
        .collect();
    assert_eq!(
        columns,
        vec![
            ("count", Some("field")),
            ("host", Some("tag")),
            ("region", Some("tag")),
            ("time", Some("time")),
            ("usage", Some("field")),
        ]
    );
    assert!(details.primary_key.is_empty() && details.row_identity().is_none());
    let missing = TableRef::new(Some(database.clone()), Some("iox".into()), "nope");
    assert!(sql.table_details(&missing).await.is_err());
    connection.close().await.unwrap();
    common::drop_database(&config, &database).await;
}

#[tokio::test]
async fn table_pages_are_newest_first_filtered_and_read_only() {
    let Some(config) = common::config() else {
        return;
    };
    let database = common::database_with(&config, &common::sample_points()).await;
    let connection = common::connect(&common::scoped(&config, &database)).await;
    let sql = common::sql(connection.as_ref());
    let cpu = TableRef::new(Some(database.clone()), Some("iox".into()), "cpu");
    let first = sql
        .fetch_table_page(
            &cpu,
            &TableQuery {
                limit: 10,
                ..TableQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(first.rows.len(), 10);
    assert!(first.has_more);
    let time = first.column_index("time").unwrap();
    assert_eq!(
        first.rows[0][time],
        Value::Text("2023-11-14T22:13:44Z".into())
    );
    assert_eq!(
        first.columns[time].type_name.as_deref(),
        Some("Timestamp(ns)")
    );
    let last = sql
        .fetch_table_page(
            &cpu,
            &TableQuery {
                offset: 20,
                limit: 10,
                ..TableQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(last.rows.len(), 5);
    assert!(!last.has_more);
    let window = "time >= '2023-11-14T22:13:40Z' AND host = 'host0'";
    let filtered = sql
        .fetch_table_page(
            &cpu,
            &TableQuery {
                filter: Some(window.into()),
                ..TableQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(filtered.rows.len(), 3);
    assert_eq!(sql.count_rows(&cpu, None).await.unwrap(), 25);
    assert_eq!(sql.count_rows(&cpu, Some(window)).await.unwrap(), 3);
    let recent = sql
        .count_rows(&cpu, Some("time > now() - INTERVAL '1 hour'"))
        .await
        .unwrap();
    assert_eq!(recent, 0);
    let queries = TableRef::new(Some(database.clone()), Some("system".into()), "queries");
    assert!(
        sql.fetch_table_page(&queries, &TableQuery::default())
            .await
            .is_ok()
    );
    let error = sql
        .apply_changes(&ChangeSet {
            table: cpu,
            changes: vec![relix_core::RowChange::Delete { key: vec![] }],
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    connection.close().await.unwrap();
    common::drop_database(&config, &database).await;
}

#[tokio::test]
async fn queries_without_a_database_are_rejected() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let error = common::sql(connection.as_ref())
        .execute("SELECT 1", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Configuration);
    connection.close().await.unwrap();
}
