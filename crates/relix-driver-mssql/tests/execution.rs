/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{StatementOutcome, Value};
use relix_driver_api::{ErrorKind, ExecuteOptions};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test]
async fn converts_engine_types() {
    let Some(config) = common::config().await else {
        return;
    };
    let connection = common::connect(&config).await;
    let batch = common::rows_of(
        common::run(
            connection.as_ref(),
            "SELECT 42 AS i, CAST(7 AS tinyint) AS t, CAST(1.5 AS float) AS f, \
             CAST(12.34 AS decimal(6,2)) AS amount, N'héllo' AS s, 0x00FF AS b, NULL AS n, \
             CAST(1 AS bit) AS flag, CAST('2026-10-23' AS date) AS d, \
             CAST('2026-10-23T12:34:56.1234567' AS datetime2) AS dt2, \
             CAST('2026-10-23T12:34:56.997' AS datetime) AS dt, \
             CAST('2026-10-23T12:34:56.5+02:00' AS datetimeoffset(3)) AS dto, \
             CAST('6F9619FF-8B86-D011-B42D-00C04FC964FF' AS uniqueidentifier) AS g, \
             CAST(12.5 AS money) AS m",
        )
        .await,
    );
    assert_eq!(
        batch.rows,
        vec![vec![
            Value::Int(42),
            Value::Int(7),
            Value::Float(1.5),
            Value::Text("12.34".into()),
            Value::Text("héllo".into()),
            Value::Bytes(vec![0x00, 0xff]),
            Value::Null,
            Value::Bool(true),
            Value::Text("2026-10-23".into()),
            Value::Text("2026-10-23 12:34:56.1234567".into()),
            Value::Text("2026-10-23 12:34:56.997".into()),
            Value::Text("2026-10-23 12:34:56.5 +02:00".into()),
            Value::Text("6F9619FF-8B86-D011-B42D-00C04FC964FF".into()),
            Value::Float(12.5),
        ]]
    );
    assert_eq!(batch.columns[3].type_name.as_deref(), Some("decimal"));
    assert_eq!(batch.columns[4].type_name.as_deref(), Some("nvarchar"));
    connection.close().await.unwrap();
}

#[tokio::test]
async fn reports_affected_rows_limits_and_errors() {
    let Some(config) = common::config().await else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let table = common::table_name("exec");
    assert_eq!(
        common::run(
            connection.as_ref(),
            &format!("CREATE TABLE [{table}] (id INT PRIMARY KEY, v INT)")
        )
        .await,
        StatementOutcome::Completed {
            message: "CREATE completed".into()
        }
    );
    assert_eq!(
        common::run(
            connection.as_ref(),
            &format!("INSERT INTO [{table}] VALUES (1, 1), (2, 2), (3, 3);")
        )
        .await,
        StatementOutcome::Affected { rows: 3 }
    );
    assert_eq!(
        common::run(
            connection.as_ref(),
            &format!("UPDATE [{table}] SET v = 9 WHERE id >= 2 -- trailing comment")
        )
        .await,
        StatementOutcome::Affected { rows: 2 }
    );
    let output = common::rows_of(
        common::run(
            connection.as_ref(),
            &format!("DELETE FROM [{table}] OUTPUT deleted.id WHERE id = 3"),
        )
        .await,
    );
    assert_eq!(output.rows, vec![vec![Value::Int(3)]]);
    let empty = common::rows_of(
        common::run(
            connection.as_ref(),
            &format!("SELECT * FROM [{table}] WHERE id > 100"),
        )
        .await,
    );
    assert!(empty.rows.is_empty());
    assert_eq!(empty.columns.len(), 2);

    let limited = common::rows_of(
        sql.execute(
            &format!("SELECT id FROM [{table}] ORDER BY id"),
            &ExecuteOptions { row_limit: 1 },
        )
        .await
        .unwrap(),
    );
    assert_eq!(limited.rows, vec![vec![Value::Int(1)]]);
    assert!(limited.has_more);

    let multiple =
        common::rows_of(common::run(connection.as_ref(), "SELECT 1 AS a; SELECT 2 AS b").await);
    assert_eq!(multiple.rows, vec![vec![Value::Int(1)]]);
    assert_eq!(multiple.columns[0].name, "a");

    let error = sql
        .execute("SELECT 1\nSELECT * FROMM t", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    assert!(
        error
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("error 102"))
    );
    assert_eq!(error.position, Some(18));
    assert_eq!(
        common::rows_of(common::run(connection.as_ref(), "SELECT 2").await).rows,
        vec![vec![Value::Int(2)]]
    );
    common::run(connection.as_ref(), &format!("DROP TABLE [{table}]")).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn keeps_session_state_and_transactions() {
    let Some(config) = common::config().await else {
        return;
    };
    let connection = common::connect(&config).await;
    let table = common::create_items(connection.as_ref()).await;
    common::run(connection.as_ref(), "CREATE TABLE #scratch (v INT)").await;
    common::run(connection.as_ref(), "INSERT INTO #scratch VALUES (17)").await;
    assert_eq!(
        common::rows_of(common::run(connection.as_ref(), "SELECT v FROM #scratch").await).rows,
        vec![vec![Value::Int(17)]]
    );
    common::run(connection.as_ref(), "BEGIN TRANSACTION").await;
    assert_eq!(
        common::run(
            connection.as_ref(),
            &format!("DELETE FROM [{table}] WHERE id = 1")
        )
        .await,
        StatementOutcome::Affected { rows: 1 }
    );
    common::run(connection.as_ref(), "ROLLBACK TRANSACTION").await;
    let count = common::sql(connection.as_ref())
        .count_rows(&common::table_ref(&config, &table), None)
        .await
        .unwrap();
    assert_eq!(count, 5);
    common::run(connection.as_ref(), &format!("DROP TABLE [{table}]")).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn cancels_a_running_statement_and_recovers() {
    let Some(config) = common::config().await else {
        return;
    };
    let connection: Arc<dyn relix_driver_api::DatabaseConnection> =
        Arc::from(common::connect(&config).await);
    let sql = common::sql(connection.as_ref());
    assert!(sql.cancel().await.is_ok());
    let running = Arc::clone(&connection);
    let started = Instant::now();
    let statement = tokio::spawn(async move {
        common::sql(running.as_ref())
            .execute("WAITFOR DELAY '00:00:30'", &ExecuteOptions::default())
            .await
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    sql.cancel().await.unwrap();
    let error = statement.await.unwrap().unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled, "{error}");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(
        common::rows_of(common::run(connection.as_ref(), "SELECT 2").await).rows,
        vec![vec![Value::Int(2)]]
    );
    connection.close().await.unwrap();
}
