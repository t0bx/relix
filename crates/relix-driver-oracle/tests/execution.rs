/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{StatementOutcome, Value};
use relix_driver_api::{DatabaseConnection, ErrorKind, ExecuteOptions};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test]
async fn converts_engine_types() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let batch = common::rows_of(
        common::run(
            connection.as_ref(),
            "SELECT 42 AS n, CAST(7 AS NUMBER(5)) AS i, 0.5 AS half, \
             CAST(12.34 AS NUMBER(6,2)) AS amount, 'héllo' AS s, HEXTORAW('00FF') AS b, \
             NULL AS nothing, DATE '2026-10-23' AS d, \
             CAST(TIMESTAMP '2026-10-23 12:34:56.123456' AS TIMESTAMP(6)) AS ts, \
             CAST(TIMESTAMP '2026-10-23 12:34:56.5 +02:00' AS TIMESTAMP(3) WITH TIME ZONE) AS tz, \
             CAST(1.5 AS BINARY_DOUBLE) AS bd, TO_CLOB('long text') AS c \
             FROM dual;",
        )
        .await,
    );
    assert_eq!(
        batch.rows,
        vec![vec![
            Value::Text("42".into()),
            Value::Int(7),
            Value::Text("0.5".into()),
            Value::Text("12.34".into()),
            Value::Text("héllo".into()),
            Value::Bytes(vec![0x00, 0xff]),
            Value::Null,
            Value::Text("2026-10-23 00:00:00".into()),
            Value::Text("2026-10-23 12:34:56.123456".into()),
            Value::Text("2026-10-23 12:34:56.500 +02:00".into()),
            Value::Float(1.5),
            Value::Text("long text".into()),
        ]]
    );
    assert_eq!(batch.columns[0].name, "N");
    assert_eq!(batch.columns[3].type_name.as_deref(), Some("NUMBER(6,2)"));
    connection.close().await.unwrap();
}

#[tokio::test]
async fn reports_affected_rows_limits_blocks_and_errors() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let conn = connection.as_ref();
    let sql = common::sql(conn);
    let table = common::object_name("exec");
    assert_eq!(
        common::run(
            conn,
            &format!("CREATE TABLE {table} (id NUMBER(10) PRIMARY KEY, v NUMBER(10))")
        )
        .await,
        StatementOutcome::Completed {
            message: "CREATE completed".into()
        }
    );
    assert_eq!(
        common::run(
            conn,
            &format!("INSERT INTO {table} SELECT level, level FROM dual CONNECT BY level <= 3;")
        )
        .await,
        StatementOutcome::Affected { rows: 3 }
    );
    assert_eq!(
        common::run(conn, &format!("UPDATE {table} SET v = 9 WHERE id >= 2\n/")).await,
        StatementOutcome::Affected { rows: 2 }
    );
    let limited = common::rows_of(
        sql.execute(
            &format!("SELECT id FROM {table} ORDER BY id"),
            &ExecuteOptions { row_limit: 2 },
        )
        .await
        .unwrap(),
    );
    assert_eq!(limited.rows, vec![vec![Value::Int(1)], vec![Value::Int(2)]]);
    assert!(limited.has_more);
    let empty =
        common::rows_of(common::run(conn, &format!("SELECT * FROM {table} WHERE id > 100")).await);
    assert!(empty.rows.is_empty());
    assert_eq!(empty.columns.len(), 2);
    assert_eq!(
        common::run(conn, "BEGIN NULL; END;").await,
        StatementOutcome::Completed {
            message: "BEGIN completed".into()
        }
    );
    let implicit = common::rows_of(
        common::run(
            conn,
            &format!(
                "DECLARE c SYS_REFCURSOR; BEGIN OPEN c FOR SELECT id FROM {table} ORDER BY id; \
                 DBMS_SQL.RETURN_RESULT(c); END;"
            ),
        )
        .await,
    );
    assert_eq!(implicit.rows.len(), 3);

    let error = sql
        .execute("SELECT *\nFROMM dual", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query, "{error}");
    assert!(
        error
            .detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with("ORA-"))
    );
    assert_eq!(error.position, Some(9), "{error}");
    common::run(conn, &format!("DROP TABLE {table} PURGE")).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn transactions_are_implicit_until_commit() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let conn = connection.as_ref();
    let sql = common::sql(conn);
    let name = common::create_items(conn).await;
    let table = common::table_ref(&config, &name);
    common::run(conn, &format!("DELETE FROM {name} WHERE id = 1")).await;
    let in_session =
        common::rows_of(common::run(conn, &format!("SELECT COUNT(*) FROM {name}")).await);
    assert_eq!(in_session.rows[0][0], Value::Text("4".into()));
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 5);
    common::run(conn, "ROLLBACK").await;
    common::run(conn, &format!("DELETE FROM {name} WHERE id = 2")).await;
    common::run(conn, "COMMIT").await;
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 4);

    common::run(conn, &format!("DELETE FROM {name} WHERE id = 3")).await;
    connection.close().await.unwrap();
    let reopened = common::connect(&config).await;
    assert_eq!(
        common::sql(reopened.as_ref())
            .count_rows(&table, None)
            .await
            .unwrap(),
        4
    );
    common::run(reopened.as_ref(), &format!("DROP TABLE {name} PURGE")).await;
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn cancels_a_running_statement() {
    let Some(config) = common::config() else {
        return;
    };
    let connection: Arc<dyn DatabaseConnection> = Arc::from(common::connect(&config).await);
    let sql = common::sql(connection.as_ref());
    assert!(sql.cancel().await.is_ok());
    let running = Arc::clone(&connection);
    let started = Instant::now();
    let statement = tokio::spawn(async move {
        common::sql(running.as_ref())
            .execute(
                "SELECT COUNT(*) FROM all_objects a, all_objects b, all_objects c",
                &ExecuteOptions::default(),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(1000)).await;
    sql.cancel().await.unwrap();
    let error = statement.await.unwrap().unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled, "{error}");
    assert!(started.elapsed() < Duration::from_secs(15));
    assert_eq!(
        common::rows_of(common::run(connection.as_ref(), "SELECT 2 FROM dual").await).rows,
        vec![vec![Value::Text("2".into())]]
    );
    connection.close().await.unwrap();
}
