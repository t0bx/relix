/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used)]

mod common;

use common::{Engine, for_each_engine};

use relix_core::{StatementOutcome, Value};
use relix_driver_api::{ErrorKind, ExecuteOptions};
use std::time::Duration;

for_each_engine!(
    select_returns_typed_rows,
    row_limit_truncates_and_flags_more_rows,
    ddl_and_dml_report_completion_and_affected_rows,
    syntax_errors_carry_a_zero_based_position,
    multi_line_syntax_errors_point_into_the_failing_line,
    session_transactions_span_statements,
    running_statements_can_be_cancelled,
);

async fn select_returns_typed_rows(engine: Engine) {
    let Some(config) = engine.config() else {
        return;
    };
    let connection = common::connect(engine, &config).await;
    let outcome = common::run(
        connection.as_ref(),
        "SELECT 1::int4 AS a, true AS b, 1.5::float8 AS c, 12.50::numeric AS d, NULL::text AS e",
    )
    .await;
    let StatementOutcome::Rows(batch) = outcome else {
        panic!("expected rows, got {outcome:?}");
    };
    let names: Vec<_> = batch
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(names, ["a", "b", "c", "d", "e"]);
    assert_eq!(batch.columns[0].type_name.as_deref(), Some("int4"));
    assert_eq!(batch.columns[3].type_name.as_deref(), Some("numeric"));
    assert_eq!(
        batch.rows,
        vec![vec![
            Value::Int(1),
            Value::Bool(true),
            Value::Float(1.5),
            Value::Text("12.50".into()),
            Value::Null,
        ]]
    );
    assert!(!batch.has_more);
    connection.close().await.unwrap();
}

async fn row_limit_truncates_and_flags_more_rows(engine: Engine) {
    let Some(config) = engine.config() else {
        return;
    };
    let connection = common::connect(engine, &config).await;
    let sql = common::sql(connection.as_ref());
    let outcome = sql
        .execute(
            "SELECT g FROM generate_series(1, 50) AS g",
            &ExecuteOptions { row_limit: 10 },
        )
        .await
        .unwrap();
    let StatementOutcome::Rows(batch) = outcome else {
        panic!("expected rows");
    };
    assert_eq!(batch.rows.len(), 10);
    assert!(batch.has_more);
    let empty = common::run(connection.as_ref(), "SELECT 1 WHERE false").await;
    assert!(
        matches!(empty, StatementOutcome::Rows(batch) if batch.rows.is_empty() && batch.columns.len() == 1)
    );
    connection.close().await.unwrap();
}

async fn ddl_and_dml_report_completion_and_affected_rows(engine: Engine) {
    let Some(config) = engine.config() else {
        return;
    };
    let connection = common::connect(engine, &config).await;
    let table = common::table_name("exec_dml");
    let created = common::run(
        connection.as_ref(),
        &format!("CREATE TABLE {table} (id int PRIMARY KEY)"),
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
        &format!("INSERT INTO {table} SELECT generate_series(1, 3)"),
    )
    .await;
    assert_eq!(inserted, StatementOutcome::Affected { rows: 3 });
    let returning = common::run(
        connection.as_ref(),
        &format!("DELETE FROM {table} WHERE id = 1 RETURNING id"),
    )
    .await;
    assert!(
        matches!(returning, StatementOutcome::Rows(batch) if batch.rows == vec![vec![Value::Int(1)]])
    );
    common::run(connection.as_ref(), &format!("DROP TABLE {table}")).await;
    connection.close().await.unwrap();
}

async fn syntax_errors_carry_a_zero_based_position(engine: Engine) {
    let Some(config) = engine.config() else {
        return;
    };
    let connection = common::connect(engine, &config).await;
    let error = common::sql(connection.as_ref())
        .execute("SELECT * FORM pg_class", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    assert_eq!(error.position, Some(9));
    connection.close().await.unwrap();
}

async fn multi_line_syntax_errors_point_into_the_failing_line(engine: Engine) {
    let Some(config) = engine.config() else {
        return;
    };
    let connection = common::connect(engine, &config).await;
    let statement = "SELECT 1,\n2 FORM pg_class";
    let error = common::sql(connection.as_ref())
        .execute(statement, &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    assert_eq!(error.position, statement.find("pg_class"), "{error:?}");
    connection.close().await.unwrap();
}

async fn session_transactions_span_statements(engine: Engine) {
    let Some(config) = engine.config() else {
        return;
    };
    let connection = common::connect(engine, &config).await;
    let table = common::table_name("exec_tx");
    let sql = common::sql(connection.as_ref());
    common::run(
        connection.as_ref(),
        &format!("CREATE TABLE {table} (id int)"),
    )
    .await;
    common::run(connection.as_ref(), "BEGIN").await;
    common::run(
        connection.as_ref(),
        &format!("INSERT INTO {table} VALUES (1)"),
    )
    .await;
    let error = sql
        .execute(
            "SELECT missing_column FROM pg_class",
            &ExecuteOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(error.message.contains("missing_column"), "{error}");
    common::run(connection.as_ref(), "ROLLBACK").await;
    let count = sql
        .count_rows(&common::table_ref(&config, &table), None)
        .await
        .unwrap();
    assert_eq!(count, 0);
    common::run(connection.as_ref(), &format!("DROP TABLE {table}")).await;
    connection.close().await.unwrap();
}

async fn running_statements_can_be_cancelled(engine: Engine) {
    let Some(config) = engine.config() else {
        return;
    };
    let connection = common::connect(engine, &config).await;
    let sql = common::sql(connection.as_ref());
    let options = ExecuteOptions::default();
    let (result, _) = tokio::join!(sql.execute("SELECT pg_sleep(30)", &options), async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        sql.cancel().await.unwrap();
    });
    assert_eq!(result.unwrap_err().kind, ErrorKind::Cancelled);
    common::run(connection.as_ref(), "SELECT 1").await;
    connection.close().await.unwrap();
}
