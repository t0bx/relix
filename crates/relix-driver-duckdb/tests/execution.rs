/*
 * @author t0bx
 * @since 23.10.2026
 */

mod common;

use relix_core::{StatementOutcome, Value};
use relix_driver_api::{ErrorKind, ExecuteOptions};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test]
async fn select_returns_typed_rows_and_duckdb_type_names() {
    let database = common::file_database();
    let connection = common::connect(&database.config).await;
    let batch = common::rows(
        connection.as_ref(),
        "SELECT 1::INTEGER AS i, true AS b, 1.5::DOUBLE AS d, 12.50::DECIMAL(10,2) AS n, \
         170141183460469231731687303715884105727::HUGEINT AS h, DATE '2024-02-29' AS day, \
         TIMESTAMP '2024-01-02 03:04:05.25' AS ts, [1, NULL] AS l, {'k': 'v w'} AS s, \
         MAP {'a': 1} AS m, '\\x01'::BLOB AS bin, NULL AS nothing",
    )
    .await;
    let types: Vec<_> = batch
        .columns
        .iter()
        .map(|column| column.type_name.as_deref().unwrap_or_default())
        .collect();
    assert_eq!(
        types,
        [
            "INTEGER",
            "BOOLEAN",
            "DOUBLE",
            "DECIMAL(10,2)",
            "HUGEINT",
            "DATE",
            "TIMESTAMP",
            "INTEGER[]",
            "STRUCT(k VARCHAR)",
            "MAP(VARCHAR, INTEGER)",
            "BLOB",
            "INTEGER"
        ]
    );
    assert_eq!(
        batch.rows,
        vec![vec![
            Value::Int(1),
            Value::Bool(true),
            Value::Float(1.5),
            Value::Text("12.50".into()),
            Value::Text("170141183460469231731687303715884105727".into()),
            Value::Text("2024-02-29".into()),
            Value::Text("2024-01-02 03:04:05.25".into()),
            Value::Text("[1, NULL]".into()),
            Value::Text("{'k': v w}".into()),
            Value::Text("{a=1}".into()),
            Value::Bytes(vec![1]),
            Value::Null,
        ]]
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn row_limit_truncates_and_flags_more_rows() {
    let database = common::file_database();
    let connection = common::connect(&database.config).await;
    let outcome = common::sql(connection.as_ref())
        .execute(
            "SELECT range AS n FROM range(1, 1000000000)",
            &ExecuteOptions { row_limit: 10 },
        )
        .await
        .unwrap();
    let StatementOutcome::Rows(batch) = outcome else {
        panic!("expected rows, got {outcome:?}");
    };
    assert_eq!(batch.rows.len(), 10);
    assert_eq!(batch.rows[9], vec![Value::Int(10)]);
    assert!(batch.has_more);
    let exact = common::sql(connection.as_ref())
        .execute("SELECT * FROM range(3)", &ExecuteOptions { row_limit: 3 })
        .await
        .unwrap();
    assert!(
        matches!(exact, StatementOutcome::Rows(batch) if !batch.has_more && batch.rows.len() == 3)
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn statements_report_affected_rows_or_completion() {
    let database = common::file_database();
    let connection = common::connect(&database.config).await;
    let create = common::run(connection.as_ref(), "CREATE TABLE t (a INTEGER)").await;
    assert_eq!(
        create,
        StatementOutcome::Completed {
            message: "CREATE TABLE".into()
        }
    );
    let insert = common::run(connection.as_ref(), "INSERT INTO t VALUES (1), (2), (3)").await;
    assert_eq!(insert, StatementOutcome::Affected { rows: 3 });
    let update = common::run(connection.as_ref(), "UPDATE t SET a = a + 10 WHERE a > 1").await;
    assert_eq!(update, StatementOutcome::Affected { rows: 2 });
    let returning =
        common::rows(connection.as_ref(), "DELETE FROM t WHERE a = 1 RETURNING a").await;
    assert_eq!(returning.rows, vec![vec![Value::Int(1)]]);
    let count = common::rows(connection.as_ref(), "SELECT count(*) AS \"Count\" FROM t").await;
    assert_eq!(count.rows, vec![vec![Value::Int(2)]]);
    let pragma = common::rows(connection.as_ref(), "PRAGMA table_info('t')").await;
    assert_eq!(pragma.rows.len(), 1);
    let set = common::run(connection.as_ref(), "SET threads = 2").await;
    assert_eq!(
        set,
        StatementOutcome::Completed {
            message: "SET".into()
        }
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn session_transactions_are_isolated_from_the_data_connection() {
    let database = common::file_database();
    let connection = common::connect(&database.config).await;
    let sql = common::sql(connection.as_ref());
    common::run(connection.as_ref(), "CREATE TABLE t (a INTEGER)").await;
    common::run(connection.as_ref(), "BEGIN TRANSACTION").await;
    common::run(connection.as_ref(), "INSERT INTO t VALUES (1)").await;
    let table = relix_core::TableRef::new(None, Some(common::SCHEMA.into()), "t");
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 0);
    common::run(connection.as_ref(), "ROLLBACK").await;
    let after_rollback = common::rows(connection.as_ref(), "SELECT count(*) FROM t").await;
    assert_eq!(after_rollback.rows, vec![vec![Value::Int(0)]]);
    common::run(connection.as_ref(), "BEGIN").await;
    common::run(connection.as_ref(), "INSERT INTO t VALUES (2)").await;
    common::run(connection.as_ref(), "COMMIT").await;
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 1);
    common::run(connection.as_ref(), "SET VARIABLE answer = 42").await;
    let variable = common::rows(connection.as_ref(), "SELECT getvariable('answer')").await;
    assert_eq!(variable.rows, vec![vec![Value::Int(42)]]);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn errors_carry_kind_and_position() {
    let database = common::file_database();
    let connection = common::connect(&database.config).await;
    let sql = common::sql(connection.as_ref());
    let options = ExecuteOptions::default();
    let syntax = sql.execute("SELECT 'a' FRM t", &options).await.unwrap_err();
    assert_eq!(syntax.kind, ErrorKind::Query);
    assert_eq!(syntax.position, Some(15));
    assert!(syntax.message.starts_with("Parser Error"));
    let catalog = sql
        .execute("SELECT *\nFROM missing_table", &options)
        .await
        .unwrap_err();
    assert_eq!(catalog.position, Some(14));
    let multiple = sql
        .execute("SELECT 1; SELECT 2", &options)
        .await
        .unwrap_err();
    assert_eq!(multiple.kind, ErrorKind::Query);
    let offset = sql
        .execute("SELECT '12:00:00+02'::TIMETZ AS t", &options)
        .await
        .unwrap_err();
    assert_eq!(offset.kind, ErrorKind::Unsupported);
    let cast = common::rows(connection.as_ref(), "SELECT '12:00:00+02'::TIMETZ::VARCHAR").await;
    assert_eq!(cast.rows, vec![vec![Value::Text("12:00:00+02".into())]]);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn cancel_interrupts_a_running_statement() {
    let database = common::file_database();
    let connection: Arc<dyn relix_driver_api::DatabaseConnection> =
        Arc::from(common::connect(&database.config).await);
    let running = Arc::clone(&connection);
    let started = Instant::now();
    let task = tokio::spawn(async move {
        common::sql(running.as_ref())
            .execute(
                "SELECT sum(hash(a.range + b.range)) FROM range(1000000) a, range(1000000) b",
                &ExecuteOptions::default(),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    common::sql(connection.as_ref()).cancel().await.unwrap();
    let error = task.await.unwrap().unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(20));
    let after = common::rows(connection.as_ref(), "SELECT 1").await;
    assert_eq!(after.rows, vec![vec![Value::Int(1)]]);
    connection.close().await.unwrap();
}
