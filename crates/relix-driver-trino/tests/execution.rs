/*
 * @author t0bx
 * @since 23.10.2026
 */

mod common;

use relix_core::{StatementOutcome, Value};
use relix_driver_api::{DatabaseConnection, ErrorKind, ExecuteOptions};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test]
async fn select_returns_typed_rows_and_type_names() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let batch = common::rows(
        connection.as_ref(),
        "SELECT 1 AS a, 9223372036854775807 AS b, DECIMAL '12345678901234567890.30' AS c, \
         nan() AS d, 'x' AS e, X'00FF' AS f, ARRAY[1, 2] AS g, MAP(ARRAY['k'], ARRAY[1]) AS h, \
         CAST(ROW(1, 'a') AS ROW(n integer, s varchar)) AS i, DATE '2024-02-29' AS j, \
         TIMESTAMP '2024-01-02 03:04:05.123456' AS k, CAST(NULL AS varchar) AS l, true AS m",
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
            "integer",
            "bigint",
            "decimal(22, 2)",
            "double",
            "varchar(1)",
            "varbinary",
            "array(integer)",
            "map(varchar(1), integer)",
            "row(n integer, s varchar)",
            "date",
            "timestamp(6)",
            "varchar",
            "boolean"
        ]
    );
    let row = &batch.rows[0];
    assert_eq!(row[0], Value::Int(1));
    assert_eq!(row[1], Value::Int(i64::MAX));
    assert_eq!(row[2], Value::Text("12345678901234567890.30".into()));
    assert!(matches!(row[3], Value::Float(value) if value.is_nan()));
    assert_eq!(row[4], Value::Text("x".into()));
    assert_eq!(row[5], Value::Bytes(vec![0, 255]));
    assert_eq!(row[6], Value::Text("[1,2]".into()));
    assert_eq!(row[7], Value::Text("{\"k\":1}".into()));
    assert!(matches!(&row[8], Value::Text(text) if text.contains('1') && text.contains("\"a\"")));
    assert_eq!(row[9], Value::Text("2024-02-29".into()));
    assert_eq!(row[10], Value::Text("2024-01-02 03:04:05.123456".into()));
    assert_eq!(row[11], Value::Null);
    assert_eq!(row[12], Value::Bool(true));
    connection.close().await.unwrap();
}

#[tokio::test]
async fn truncated_results_abandon_the_query() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let marker = common::table_name("truncate");
    let outcome = common::sql(connection.as_ref())
        .execute(
            &format!("SELECT orderkey, '{marker}' FROM tpch.sf1.lineitem"),
            &ExecuteOptions { row_limit: 10 },
        )
        .await
        .unwrap();
    let StatementOutcome::Rows(batch) = outcome else {
        panic!("expected rows, got {outcome:?}");
    };
    assert_eq!(batch.rows.len(), 10);
    assert!(batch.has_more);
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let states = common::rows(
            connection.as_ref(),
            &format!(
                "SELECT state FROM system.runtime.queries \
                 WHERE query LIKE '%{marker}%' AND query NOT LIKE '%system.runtime%'"
            ),
        )
        .await;
        let finished = !states.rows.is_empty()
            && states.rows.iter().all(|row| {
                matches!(&row[0], Value::Text(state) if state == "FAILED" || state == "FINISHED")
            });
        if finished {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the abandoned query kept running: {states:?}"
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    connection.close().await.unwrap();
}

#[tokio::test]
async fn session_changes_apply_to_later_statements() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let start = common::rows(
        connection.as_ref(),
        "SELECT current_catalog, current_schema",
    )
    .await;
    assert_eq!(
        start.rows,
        vec![vec![Value::Text("tpch".into()), Value::Text("tiny".into())]]
    );
    let used = common::run(connection.as_ref(), "USE memory.default").await;
    assert_eq!(
        used,
        StatementOutcome::Completed {
            message: "USE".into()
        }
    );
    let moved = common::rows(
        connection.as_ref(),
        "SELECT current_catalog, current_schema",
    )
    .await;
    assert_eq!(
        moved.rows,
        vec![vec![
            Value::Text("memory".into()),
            Value::Text("default".into())
        ]]
    );
    common::run(
        connection.as_ref(),
        "SET SESSION query_max_run_time = '17m'",
    )
    .await;
    let session = common::rows(
        connection.as_ref(),
        "SHOW SESSION LIKE 'query_max_run_time'",
    )
    .await;
    assert_eq!(session.rows[0][1], Value::Text("17m".into()));
    common::run(connection.as_ref(), "RESET SESSION query_max_run_time").await;
    let reset = common::rows(
        connection.as_ref(),
        "SHOW SESSION LIKE 'query_max_run_time'",
    )
    .await;
    assert_ne!(reset.rows[0][1], Value::Text("17m".into()));
    common::run(connection.as_ref(), "PREPARE add_one FROM SELECT ? + 1").await;
    let executed = common::rows(connection.as_ref(), "EXECUTE add_one USING 41").await;
    assert_eq!(executed.rows, vec![vec![Value::Int(42)]]);
    common::run(connection.as_ref(), "DEALLOCATE PREPARE add_one").await;
    let gone = common::sql(connection.as_ref())
        .execute("EXECUTE add_one USING 1", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(gone.kind, ErrorKind::Query);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn statements_report_affected_rows_or_completion() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let table = format!("memory.default.{}", common::table_name("events"));
    let created = common::run(
        connection.as_ref(),
        &format!("CREATE TABLE {table} (id bigint, label varchar)"),
    )
    .await;
    assert_eq!(
        created,
        StatementOutcome::Completed {
            message: "CREATE TABLE".into()
        }
    );
    let inserted = common::run(
        connection.as_ref(),
        &format!("INSERT INTO {table} VALUES (1, 'a'), (2, 'b'), (3, 'c');"),
    )
    .await;
    assert_eq!(inserted, StatementOutcome::Affected { rows: 3 });
    let copied = common::run(
        connection.as_ref(),
        &format!("CREATE TABLE {table}_copy AS SELECT * FROM {table} WHERE id > 1"),
    )
    .await;
    assert_eq!(copied, StatementOutcome::Affected { rows: 2 });
    let rows = common::rows(
        connection.as_ref(),
        &format!("SELECT count(*) FROM {table}"),
    )
    .await;
    assert_eq!(rows.rows, vec![vec![Value::Int(3)]]);
    common::run(connection.as_ref(), &format!("DROP TABLE {table}_copy")).await;
    common::run(connection.as_ref(), &format!("DROP TABLE {table}")).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn errors_carry_kind_and_position() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let options = ExecuteOptions::default();
    let syntax = sql
        .execute("SELECT 'ä'\nFRM nation", &options)
        .await
        .unwrap_err();
    assert_eq!(syntax.kind, ErrorKind::Query);
    assert_eq!(syntax.position, Some(15));
    assert_eq!(syntax.detail.as_deref(), Some("SYNTAX_ERROR"));
    let missing = sql
        .execute("SELECT * FROM relix_missing_table", &options)
        .await
        .unwrap_err();
    assert_eq!(missing.kind, ErrorKind::Query);
    assert_eq!(missing.detail.as_deref(), Some("TABLE_NOT_FOUND"));
    assert_eq!(missing.position, Some(14));
    let multiple = sql
        .execute("SELECT 1; SELECT 2", &options)
        .await
        .unwrap_err();
    assert_eq!(multiple.kind, ErrorKind::Query);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn cancel_stops_the_running_statement() {
    let Some(config) = common::config() else {
        return;
    };
    let connection: Arc<dyn DatabaseConnection> = Arc::from(common::connect(&config).await);
    let running = Arc::clone(&connection);
    let started = Instant::now();
    let task = tokio::spawn(async move {
        common::sql(running.as_ref())
            .execute(
                "SELECT count(*) FROM tpch.sf100.lineitem",
                &ExecuteOptions::default(),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(1500)).await;
    common::sql(connection.as_ref()).cancel().await.unwrap();
    let error = task.await.unwrap().unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled, "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(30));
    let after = common::rows(connection.as_ref(), "SELECT 1").await;
    assert_eq!(after.rows, vec![vec![Value::Int(1)]]);
    connection.close().await.unwrap();
}
