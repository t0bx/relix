/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{StatementOutcome, Value};
use relix_driver_api::{ErrorKind, ExecuteOptions};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn pages_results_up_to_the_row_limit() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let keyspace = common::create_keyspace(connection.as_ref()).await;
    common::run(
        connection.as_ref(),
        &format!("CREATE TABLE {keyspace}.events (bucket int, seq int, payload text, PRIMARY KEY (bucket, seq))"),
    )
    .await;
    for seq in 0..25 {
        common::run(
            connection.as_ref(),
            &format!("INSERT INTO {keyspace}.events (bucket, seq, payload) VALUES (1, {seq}, 'p{seq}')"),
        )
        .await;
    }
    let sql = common::sql(connection.as_ref());
    let select = format!("SELECT seq, payload FROM {keyspace}.events WHERE bucket = 1;");
    let limited = sql
        .execute(&select, &ExecuteOptions { row_limit: 10 })
        .await
        .unwrap();
    let batch = common::rows(limited);
    assert_eq!(batch.rows.len(), 10);
    assert!(batch.has_more);
    assert_eq!(batch.columns[0].name, "seq");
    assert_eq!(batch.columns[0].type_name.as_deref(), Some("int"));
    assert_eq!(batch.rows[3], vec![Value::Int(3), Value::Text("p3".into())]);
    let everything = common::rows(
        sql.execute(&select, &ExecuteOptions { row_limit: 25 })
            .await
            .unwrap(),
    );
    assert_eq!(everything.rows.len(), 25);
    assert!(!everything.has_more);
    let empty = common::rows(
        sql.execute(
            &format!("SELECT * FROM {keyspace}.events WHERE bucket = 2"),
            &ExecuteOptions::default(),
        )
        .await
        .unwrap(),
    );
    assert!(empty.rows.is_empty());
    assert_eq!(empty.columns.len(), 3);
    common::drop_keyspace(connection.as_ref(), &keyspace).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn converts_cql_types_to_grid_values() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let keyspace = common::create_keyspace(connection.as_ref()).await;
    common::run(
        connection.as_ref(),
        &format!(
            "CREATE TABLE {keyspace}.typed (id uuid PRIMARY KEY, flag boolean, amount decimal, \
             big varint, ratio float, at timestamp, day date, tags set<text>, attrs map<text, int>, \
             data blob, addr inet)"
        ),
    )
    .await;
    let id = "123e4567-e89b-12d3-a456-426614174000";
    common::run(
        connection.as_ref(),
        &format!(
            "INSERT INTO {keyspace}.typed (id, flag, amount, big, ratio, at, day, tags, attrs, data, addr) \
             VALUES ({id}, true, 12.50, 123456789012345678901234567890, 0.1, \
             '2024-05-06 07:08:09.123+0000', '2024-05-06', {{'b', 'a'}}, {{'k': 1}}, 0xcafe, '10.0.0.1')"
        ),
    )
    .await;
    let batch = common::rows(
        common::run(
            connection.as_ref(),
            &format!(
                "SELECT id, flag, amount, big, ratio, at, day, tags, attrs, data, addr FROM {keyspace}.typed"
            ),
        )
        .await,
    );
    assert_eq!(
        batch.rows[0],
        vec![
            Value::Text(id.into()),
            Value::Bool(true),
            Value::Text("12.50".into()),
            Value::Text("123456789012345678901234567890".into()),
            Value::Float(0.1),
            Value::Text("2024-05-06 07:08:09.123+0000".into()),
            Value::Text("2024-05-06".into()),
            Value::Text("{'a','b'}".into()),
            Value::Text("{'k':1}".into()),
            Value::Bytes(vec![0xca, 0xfe]),
            Value::Text("10.0.0.1".into()),
        ]
    );
    assert_eq!(batch.columns[7].type_name.as_deref(), Some("set<text>"));
    common::drop_keyspace(connection.as_ref(), &keyspace).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn statements_without_rows_complete_and_use_switches_keyspace() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let keyspace = common::create_keyspace(connection.as_ref()).await;
    let outcome = common::run(connection.as_ref(), &format!("USE {keyspace}")).await;
    assert!(matches!(outcome, StatementOutcome::Completed { .. }));
    let created = common::run(
        connection.as_ref(),
        "CREATE TABLE notes (id int PRIMARY KEY, body text)",
    )
    .await;
    assert_eq!(
        created,
        StatementOutcome::Completed {
            message: "CREATE TABLE executed".into()
        }
    );
    common::run(
        connection.as_ref(),
        "INSERT INTO notes (id, body) VALUES (1, 'one')",
    )
    .await;
    let conditional = common::rows(
        common::run(
            connection.as_ref(),
            "INSERT INTO notes (id, body) VALUES (1, 'again') IF NOT EXISTS",
        )
        .await,
    );
    assert_eq!(conditional.columns[0].name, "[applied]");
    assert_eq!(conditional.rows[0][0], Value::Bool(false));
    common::drop_keyspace(connection.as_ref(), &keyspace).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn syntax_errors_point_at_the_statement() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let error = common::sql(connection.as_ref())
        .execute("SELECT * FRM system.local", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query, "{error}");
    assert!(error.position.is_some(), "{error:?}");
    let missing = common::sql(connection.as_ref())
        .execute(
            "SELECT * FROM system.no_such_table",
            &ExecuteOptions::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(missing.kind, ErrorKind::Query, "{missing}");
    connection.close().await.unwrap();
}

#[tokio::test]
async fn cancel_stops_waiting_for_a_running_statement() {
    let Some(config) = common::config() else {
        return;
    };
    let connection: Arc<dyn relix_driver_api::DatabaseConnection> =
        Arc::from(common::connect(&config).await);
    let running = {
        let connection = connection.clone();
        tokio::spawn(async move {
            common::sql(connection.as_ref())
                .execute(
                    "SELECT * FROM system_schema.columns",
                    &ExecuteOptions { row_limit: 1_000_000 },
                )
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(1)).await;
    common::sql(connection.as_ref()).cancel().await.unwrap();
    match running.await.unwrap() {
        Err(error) => assert_eq!(error.kind, ErrorKind::Cancelled),
        Ok(_) => eprintln!("statement finished before the cancel request arrived"),
    }
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
}
