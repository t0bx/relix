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
        "SELECT toUInt8(1) AS a, toInt64(-5) AS b, toUInt64(18446744073709551615) AS c, \
         toDecimal128('12345678901234567890.123', 3) AS d, toFloat64(1.5) AS e, nan AS f, \
         'x\"y' AS g, [1, 2] AS h, map('k', 1) AS i, NULL AS j, true AS k, \
         toDate('2024-02-29') AS l, toInt256('-57896044618658097711785492504343953926634992332820282019728792003956564819968') AS m",
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
            "UInt8",
            "Int64",
            "UInt64",
            "Decimal(38, 3)",
            "Float64",
            "Float64",
            "String",
            "Array(UInt8)",
            "Map(String, UInt8)",
            "Nullable(Nothing)",
            "Bool",
            "Date",
            "Int256"
        ]
    );
    let row = &batch.rows[0];
    assert_eq!(row[0], Value::Int(1));
    assert_eq!(row[1], Value::Int(-5));
    assert_eq!(row[2], Value::UInt(u64::MAX));
    assert_eq!(row[3], Value::Text("12345678901234567890.123".into()));
    assert_eq!(row[4], Value::Float(1.5));
    assert!(matches!(row[5], Value::Float(value) if value.is_nan()));
    assert_eq!(row[6], Value::Text("x\"y".into()));
    assert_eq!(row[7], Value::Text("[1,2]".into()));
    assert_eq!(row[8], Value::Text("{\"k\":1}".into()));
    assert_eq!(row[9], Value::Null);
    assert_eq!(row[10], Value::Bool(true));
    assert_eq!(row[11], Value::Text("2024-02-29".into()));
    assert_eq!(
        row[12],
        Value::Text(
            "-57896044618658097711785492504343953926634992332820282019728792003956564819968".into()
        )
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn truncated_results_stop_the_query() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let marker = common::table_name("endless");
    let outcome = common::sql(connection.as_ref())
        .execute(
            &format!("SELECT number, '{marker}' FROM system.numbers"),
            &ExecuteOptions { row_limit: 10 },
        )
        .await
        .unwrap();
    let StatementOutcome::Rows(batch) = outcome else {
        panic!("expected rows, got {outcome:?}");
    };
    assert_eq!(batch.rows.len(), 10);
    assert!(batch.has_more);
    assert_eq!(batch.rows[9][0], Value::UInt(9));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let running = common::rows(
            connection.as_ref(),
            &format!(
                "SELECT count() FROM system.processes WHERE query LIKE '%{marker}%' \
                 AND query NOT LIKE '%system.processes%'"
            ),
        )
        .await;
        if running.rows[0][0] == Value::UInt(0) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the truncated query kept running"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    connection.close().await.unwrap();
}

#[tokio::test]
async fn session_state_persists_between_statements() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let initial = common::rows(connection.as_ref(), "SELECT currentDatabase()").await;
    assert_eq!(
        initial.rows,
        vec![vec![Value::Text(common::database(&config))]]
    );
    let scratch = common::table_name("relix_scratch");
    let create = common::run(connection.as_ref(), &format!("CREATE DATABASE {scratch}")).await;
    assert_eq!(
        create,
        StatementOutcome::Completed {
            message: "CREATE DATABASE".into()
        }
    );
    common::run(connection.as_ref(), &format!("USE {scratch}")).await;
    let current = common::rows(connection.as_ref(), "SELECT currentDatabase()").await;
    assert_eq!(current.rows, vec![vec![Value::Text(scratch.clone())]]);
    common::run(connection.as_ref(), "SET max_threads = 3").await;
    let setting = common::rows(connection.as_ref(), "SELECT getSetting('max_threads')").await;
    assert_eq!(setting.rows[0][0].to_plain_string().as_deref(), Some("3"));
    common::run(
        connection.as_ref(),
        "CREATE TEMPORARY TABLE scratchpad (a UInt32)",
    )
    .await;
    let inserted = common::run(
        connection.as_ref(),
        "INSERT INTO scratchpad SELECT number FROM numbers(4)",
    )
    .await;
    assert_eq!(inserted, StatementOutcome::Affected { rows: 4 });
    let count = common::rows(connection.as_ref(), "SELECT count() FROM scratchpad").await;
    assert_eq!(count.rows, vec![vec![Value::UInt(4)]]);
    common::run(connection.as_ref(), &format!("DROP DATABASE {scratch}")).await;
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
        .execute("SELECT 'ä' FRM t WHERE", &options)
        .await
        .unwrap_err();
    assert_eq!(syntax.kind, ErrorKind::Query);
    assert_eq!(syntax.position, Some(15));
    assert!(syntax.message.contains("SYNTAX_ERROR"));
    let missing = sql
        .execute("SELECT * FROM relix_absent_table", &options)
        .await
        .unwrap_err();
    assert_eq!(missing.kind, ErrorKind::Query);
    assert!(
        missing.message.contains("UNKNOWN_TABLE"),
        "{}",
        missing.message
    );
    let wide_limit = ExecuteOptions { row_limit: 100_000 };
    let streaming = sql
        .execute(
            "SELECT number, throwIf(number = 3000) FROM numbers(1000000) \
             SETTINGS max_block_size = 1000",
            &wide_limit,
        )
        .await
        .err()
        .unwrap();
    assert_eq!(streaming.kind, ErrorKind::Query);
    assert!(
        streaming.message.contains("throwIf"),
        "{}",
        streaming.message
    );
    let single_column = sql
        .execute(
            "SELECT throwIf(number = 3000) FROM numbers(1000000) SETTINGS max_block_size = 1000",
            &wide_limit,
        )
        .await
        .err()
        .unwrap();
    assert!(
        single_column
            .message
            .contains("FUNCTION_THROW_IF_VALUE_IS_NON_ZERO")
    );
    let multiple = sql
        .execute("SELECT 1; SELECT 2", &options)
        .await
        .unwrap_err();
    assert_eq!(multiple.kind, ErrorKind::Query);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn explicit_formats_are_returned_as_text() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let batch = common::rows(
        connection.as_ref(),
        "SELECT number, number * 2 FROM numbers(3) FORMAT CSV",
    )
    .await;
    assert_eq!(batch.columns[0].name, "result");
    assert_eq!(
        batch.rows,
        vec![
            vec![Value::Text("0,0".into())],
            vec![Value::Text("1,2".into())],
            vec![Value::Text("2,4".into())]
        ]
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn cancel_kills_the_running_statement() {
    let Some(config) = common::config() else {
        return;
    };
    let connection: Arc<dyn DatabaseConnection> = Arc::from(common::connect(&config).await);
    let running = Arc::clone(&connection);
    let started = Instant::now();
    let task = tokio::spawn(async move {
        common::sql(running.as_ref())
            .execute(
                "SELECT count() FROM numbers(100000000000)",
                &ExecuteOptions::default(),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(800)).await;
    common::sql(connection.as_ref()).cancel().await.unwrap();
    let error = task.await.unwrap().unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(30));
    let after = common::rows(connection.as_ref(), "SELECT 1").await;
    assert_eq!(after.rows, vec![vec![Value::Int(1)]]);
    connection.close().await.unwrap();
}
