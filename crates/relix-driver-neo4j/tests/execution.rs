/*
 * @author t0bx
 * @since 24.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{StatementOutcome, Value};
use relix_driver_api::{DatabaseConnection, ErrorKind, ExecuteOptions};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn results_keep_the_projection_order_and_respect_the_row_limit() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let label = common::label("Item");
    common::run(
        connection.as_ref(),
        &format!("UNWIND range(1, 25) AS i CREATE (:`{label}` {{seq: i, name: 'item ' + i}})"),
    )
    .await;
    let statement = format!(
        "MATCH (n:`{label}`) RETURN n.seq AS zeta, n.name AS alpha, n.seq * 2 ORDER BY zeta"
    );
    let batch = match common::sql(connection.as_ref())
        .execute(&statement, &ExecuteOptions { row_limit: 10 })
        .await
        .unwrap()
    {
        StatementOutcome::Rows(batch) => batch,
        other => panic!("{other:?}"),
    };
    let names: Vec<&str> = batch
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(names, ["zeta", "alpha", "n.seq * 2"]);
    assert_eq!(batch.columns[0].type_name.as_deref(), Some("Integer"));
    assert_eq!(batch.rows.len(), 10);
    assert!(batch.has_more);
    assert_eq!(
        batch.rows[0],
        vec![Value::Int(1), Value::Text("item 1".into()), Value::Int(2)]
    );
    let empty = common::rows(
        connection.as_ref(),
        &format!("MATCH (n:`{label}`) WHERE n.seq > 100 RETURN n.name AS name, n"),
    )
    .await;
    assert!(empty.rows.is_empty());
    assert_eq!(empty.columns.len(), 2);
    common::remove_label(connection.as_ref(), &label).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn graph_values_render_as_json_and_temporals_as_iso_text() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let label = common::label("Person");
    let batch = common::rows(
        connection.as_ref(),
        &format!(
            "CREATE (a:`{label}` {{name: 'Ada', tags: ['x', 'y']}})-[k:KNOWS {{since: 2020}}]->\
             (b:`{label}` {{name: 'Bob'}}) \
             RETURN a, k, [a.name, b.name] AS names, {{answer: 42}} AS map, \
             date('2024-05-06') AS day, datetime('2024-05-06T07:08:09Z') AS moment, \
             duration({{months: 1, days: 2, seconds: 3}}) AS span, point({{x: 1.5, y: 2.0}}) AS location"
        ),
    )
    .await;
    let row = &batch.rows[0];
    let node: serde_json::Value =
        serde_json::from_str(row[common::column(&batch, "a")].as_text().unwrap()).unwrap();
    assert_eq!(node["labels"][0], label.as_str());
    assert_eq!(node["properties"]["tags"], serde_json::json!(["x", "y"]));
    assert!(node["id"].is_i64());
    let relationship: serde_json::Value =
        serde_json::from_str(row[common::column(&batch, "k")].as_text().unwrap()).unwrap();
    assert_eq!(relationship["type"], "KNOWS");
    assert_eq!(relationship["properties"]["since"], 2020);
    assert_eq!(relationship["start"], node["id"]);
    assert_eq!(
        row[common::column(&batch, "names")],
        Value::Text(r#"["Ada","Bob"]"#.into())
    );
    assert_eq!(
        row[common::column(&batch, "map")],
        Value::Text(r#"{"answer":42}"#.into())
    );
    assert_eq!(
        row[common::column(&batch, "day")],
        Value::Text("2024-05-06".into())
    );
    assert_eq!(
        row[common::column(&batch, "moment")],
        Value::Text("2024-05-06T07:08:09+00:00".into())
    );
    assert_eq!(
        row[common::column(&batch, "span")],
        Value::Text("P1M2DT3S".into())
    );
    assert_eq!(
        row[common::column(&batch, "location")],
        Value::Text(r#"{"srid":7203,"x":1.5,"y":2.0}"#.into())
    );
    let path = common::rows(
        connection.as_ref(),
        &format!("MATCH p = (:`{label}`)-[:KNOWS]->(:`{label}`) RETURN p"),
    )
    .await;
    let path: serde_json::Value = serde_json::from_str(path.rows[0][0].as_text().unwrap()).unwrap();
    assert_eq!(path["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(path["relationships"][0]["type"], "KNOWS");
    common::remove_label(connection.as_ref(), &label).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn writes_without_projection_complete_and_errors_have_positions() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let label = common::label("Thing");
    let outcome = common::run(connection.as_ref(), &format!("CREATE (:`{label}`);")).await;
    assert_eq!(
        outcome,
        StatementOutcome::Completed {
            message: "statement executed".into()
        }
    );
    let error = common::sql(connection.as_ref())
        .execute("MATCH (n) RETRN n", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query, "{error}");
    assert_eq!(error.position, Some(10), "{error:?}");
    assert!(error.detail.unwrap().contains("SyntaxError"));
    common::remove_label(connection.as_ref(), &label).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn explicit_transactions_commit_and_roll_back() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let label = common::label("Txn");
    let count = format!("MATCH (n:`{label}`) RETURN count(n) AS c");
    common::run(connection.as_ref(), "BEGIN").await;
    common::run(connection.as_ref(), &format!("CREATE (:`{label}`)")).await;
    assert_eq!(
        common::rows(connection.as_ref(), &count).await.rows[0][0],
        Value::Int(1)
    );
    let error = common::sql(connection.as_ref())
        .execute("BEGIN", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    common::run(connection.as_ref(), "ROLLBACK").await;
    assert_eq!(
        common::rows(connection.as_ref(), &count).await.rows[0][0],
        Value::Int(0)
    );

    common::run(connection.as_ref(), "begin transaction").await;
    common::run(connection.as_ref(), &format!("CREATE (:`{label}`)")).await;
    let other = common::connect(&config).await;
    assert_eq!(
        common::rows(other.as_ref(), &count).await.rows[0][0],
        Value::Int(0)
    );
    common::run(connection.as_ref(), "COMMIT").await;
    assert_eq!(
        common::rows(other.as_ref(), &count).await.rows[0][0],
        Value::Int(1)
    );

    common::run(connection.as_ref(), "BEGIN").await;
    let failed = common::sql(connection.as_ref())
        .execute("RETURN 1/0", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert!(failed.detail.unwrap().contains("rolled back"));
    let error = common::sql(connection.as_ref())
        .execute("COMMIT", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    common::remove_label(connection.as_ref(), &label).await;
    other.close().await.unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
async fn cancel_terminates_the_running_statement_on_the_server() {
    let Some(config) = common::config() else {
        return;
    };
    let connection: Arc<dyn DatabaseConnection> = Arc::from(common::connect(&config).await);
    let running = {
        let connection = connection.clone();
        tokio::spawn(async move {
            common::sql(connection.as_ref())
                .execute(
                    "UNWIND range(1, 2000000000) AS x WITH x WHERE x < 0 RETURN count(x) AS c",
                    &ExecuteOptions::default(),
                )
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(1500)).await;
    common::sql(connection.as_ref()).cancel().await.unwrap();
    let error = running.await.unwrap().unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled, "{error}");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let remaining = common::rows(
        connection.as_ref(),
        "SHOW TRANSACTIONS YIELD currentQuery WHERE currentQuery CONTAINS ('20000' + '00000') \
         RETURN count(*) AS running",
    )
    .await;
    assert_eq!(remaining.rows[0][0], Value::Int(0));
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
}
