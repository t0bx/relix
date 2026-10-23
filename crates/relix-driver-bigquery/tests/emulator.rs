/*
 * @author t0bx
 * @since 24.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

use relix_core::{
    ConnectionConfig, ConnectionSettings, ObjectKind, StatementOutcome, TableQuery, TableRef,
    TlsMode, TlsSettings, Value,
};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, ErrorKind, ExecuteOptions};
use relix_driver_bigquery::BigQueryDriver;
use relix_driver_http::{HttpAuth, HttpEndpoint, Method, Url};
use relix_testing::integration::{integration_enabled, unique_name};
use serde_json::json;
use std::env;

const PROJECT: &str = "relix-test";

fn endpoint() -> String {
    env::var("RELIX_TEST_BIGQUERY_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:59050".into())
}

/// Settings for the `bigquery` service of `docker-compose.test.yml`
/// (`ghcr.io/goccy/bigquery-emulator --project=relix-test`).
fn emulator_config() -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping BigQuery emulator test: set RELIX_INTEGRATION=1");
        return None;
    }
    let mut settings = ConnectionSettings {
        connect_timeout_secs: 30,
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        ..ConnectionSettings::default()
    };
    for (name, value) in [
        ("endpoint", endpoint()),
        ("auth", "none".to_owned()),
        ("project_id", PROJECT.to_owned()),
    ] {
        settings.properties.insert(name.into(), value);
    }
    Some(ConnectionConfig::new(settings, None))
}

/// The emulator accepts `CREATE SCHEMA` without creating a dataset, so test datasets are
/// managed through the REST `datasets` collection.
async fn datasets_api(config: &ConnectionConfig) -> HttpEndpoint {
    let base = Url::parse(&format!("{}/bigquery/v2/projects/{PROJECT}/", endpoint())).unwrap();
    HttpEndpoint::at_base_url(config, base, HttpAuth::None, None)
        .await
        .unwrap()
}

async fn execute(connection: &dyn DatabaseConnection, sql: &str, limit: usize) -> StatementOutcome {
    connection
        .as_sql()
        .unwrap()
        .execute(sql, &ExecuteOptions { row_limit: limit })
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
}

#[tokio::test]
async fn emulator_round_trip() {
    let Some(config) = emulator_config() else {
        return;
    };
    let dataset = unique_name("relix");
    let api = datasets_api(&config).await;
    api.send(api.request(Method::POST, "datasets").json(&json!({
        "datasetReference": {"projectId": PROJECT, "datasetId": dataset}
    })))
    .await
    .unwrap();

    let connection = BigQueryDriver::new().connect(&config).await.unwrap();
    connection.ping().await.unwrap();
    let created = execute(
        connection.as_ref(),
        &format!(
            "CREATE TABLE {dataset}.orders (id INT64 NOT NULL, name STRING, price NUMERIC, \
             created TIMESTAMP, tags ARRAY<STRING>, meta STRUCT<a INT64, b STRING>, raw BYTES)"
        ),
        10,
    )
    .await;
    assert!(matches!(created, StatementOutcome::Completed { .. }));
    let inserted = execute(
        connection.as_ref(),
        &format!(
            "INSERT INTO {dataset}.orders (id, name, price, created, tags, meta, raw) VALUES \
             (1, 'Ada', NUMERIC '12.50', TIMESTAMP '2024-01-02 03:04:05.123456 UTC', ['a', 'b'], \
             STRUCT(1 AS a, 'x' AS b), b'\\x01\\xff'), \
             (2, NULL, NULL, NULL, [], NULL, NULL), \
             (3, 'Linus', NUMERIC '1', NULL, ['c'], STRUCT(2 AS a, NULL AS b), NULL)"
        ),
        10,
    )
    .await;
    assert!(matches!(
        inserted,
        StatementOutcome::Affected { rows: 3 } | StatementOutcome::Completed { .. }
    ));

    let selected = execute(
        connection.as_ref(),
        &format!("SELECT * FROM {dataset}.orders ORDER BY id"),
        2,
    )
    .await;
    let StatementOutcome::Rows(batch) = selected else {
        panic!("expected rows, got {selected:?}");
    };
    assert_eq!(batch.rows.len(), 2);
    assert!(batch.has_more);
    let types: Vec<&str> = batch
        .columns
        .iter()
        .map(|column| column.type_name.as_deref().unwrap_or_default())
        .collect();
    assert_eq!(
        types,
        vec![
            "INT64",
            "STRING",
            "NUMERIC",
            "TIMESTAMP",
            "ARRAY<STRING>",
            "STRUCT<a INT64, b STRING>",
            "BYTES"
        ]
    );
    assert_eq!(
        batch.rows[0],
        vec![
            Value::Int(1),
            Value::Text("Ada".into()),
            Value::Text("12.5".into()),
            Value::Text("2024-01-02 03:04:05.123456 UTC".into()),
            Value::Text(r#"["a","b"]"#.into()),
            Value::Text(r#"{"a":1,"b":"x"}"#.into()),
            Value::Bytes(vec![1, 255]),
        ]
    );
    assert_eq!(batch.rows[1][1], Value::Null);

    let error = connection
        .as_sql()
        .unwrap()
        .execute("SELEC 1", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    assert_eq!(error.position, Some(0));

    let sql = connection.as_sql().unwrap();
    assert!(sql.schemas(PROJECT).await.unwrap().contains(&dataset));
    let objects = sql.objects(PROJECT, Some(&dataset)).await.unwrap();
    assert!(
        objects
            .iter()
            .any(|object| object.object.name == "orders" && object.kind == ObjectKind::Table)
    );
    let table = TableRef::new(Some(PROJECT.into()), Some(dataset.clone()), "orders");
    let details = sql.table_details(&table).await.unwrap();
    assert_eq!(details.columns.len(), 7);
    assert_eq!(details.columns[4].data_type, "ARRAY<STRING>");

    let stored = sql
        .fetch_table_page(
            &table,
            &TableQuery {
                limit: 2,
                ..TableQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(stored.rows.len(), 2);
    assert!(stored.has_more);
    let filtered = sql
        .fetch_table_page(
            &table,
            &TableQuery {
                filter: Some("id > 1".into()),
                sort: vec![relix_core::SortKey {
                    column: "id".into(),
                    direction: relix_core::SortDirection::Descending,
                }],
                limit: 10,
                ..TableQuery::default()
            },
        )
        .await
        .unwrap();
    let ids: Vec<&Value> = filtered.rows.iter().map(|row| &row[0]).collect();
    assert_eq!(ids, vec![&Value::Int(3), &Value::Int(2)]);
    assert!(!filtered.has_more);
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 3);
    assert_eq!(sql.count_rows(&table, Some("id > 1")).await.unwrap(), 2);

    execute(
        connection.as_ref(),
        &format!("DROP TABLE {dataset}.orders"),
        10,
    )
    .await;
    connection.close().await.unwrap();
    api.send(api.request(Method::DELETE, &format!("datasets/{dataset}")))
        .await
        .unwrap();
}
