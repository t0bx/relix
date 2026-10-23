/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use relix_core::{StatementOutcome, Value};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, ErrorKind, ExecuteOptions};
use relix_driver_snowflake::SnowflakeDriver;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use wiremock::matchers::{
    body_partial_json, method, path, query_param as query_param_matches, query_param_is_missing,
};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const HANDLE: &str = "01b2-handle";
const STATUS_PATH: &str = "/api/v2/statements/01b2-handle";
const CANCEL_PATH: &str = "/api/v2/statements/01b2-handle/cancel";

async fn connect(server: &MockServer) -> Box<dyn DatabaseConnection> {
    Mock::given(method("POST"))
        .and(path("/api/v2/statements"))
        .and(body_partial_json(
            json!({"statement": "SELECT CURRENT_VERSION()"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(version_result()))
        .with_priority(1)
        .mount(server)
        .await;
    let mut config = key_pair_config(server);
    config.settings.query_timeout_secs = Some(30);
    SnowflakeDriver::new().connect(&config).await.unwrap()
}

async fn submit_answers(server: &MockServer, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/api/v2/statements"))
        .respond_with(response)
        .with_priority(5)
        .mount(server)
        .await;
}

async fn requests_to(server: &MockServer, method: &str, path: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.method.as_str() == method && request.url.path() == path)
        .collect()
}

fn options(row_limit: usize) -> ExecuteOptions {
    ExecuteOptions { row_limit }
}

fn orders_columns() -> Vec<serde_json::Value> {
    vec![
        column("ID", "fixed"),
        column("NAME", "text"),
        json!({"name": "CREATED", "type": "timestamp_ntz", "scale": 3, "nullable": true}),
        column("PAID", "boolean"),
    ]
}

#[tokio::test]
async fn async_statements_are_polled_and_partitions_fetched_up_to_the_limit() {
    let server = MockServer::start().await;
    let connection = connect(&server).await;
    submit_answers(
        &server,
        ResponseTemplate::new(202).set_body_json(running(HANDLE)),
    )
    .await;
    Mock::given(method("GET"))
        .and(path(STATUS_PATH))
        .and(query_param_is_missing("partition"))
        .respond_with(ResponseTemplate::new(202).set_body_json(running(HANDLE)))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    let first = result_set(
        orders_columns(),
        json!([
            ["1", "Ada", "1616173619.500000000", "true"],
            ["2", null, null, "false"]
        ]),
        6,
        3,
    );
    Mock::given(method("GET"))
        .and(path(STATUS_PATH))
        .and(query_param_is_missing("partition"))
        .respond_with(ResponseTemplate::new(200).set_body_json(first))
        .with_priority(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(STATUS_PATH))
        .and(query_param_matches("partition", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [["3", "Grace", "0.000000000", "true"], ["4", "Linus", null, null]]
        })))
        .mount(&server)
        .await;

    let sql = connection.as_sql().unwrap();
    let outcome = sql
        .execute("SELECT * FROM orders", &options(3))
        .await
        .unwrap();
    let StatementOutcome::Rows(batch) = outcome else {
        panic!("expected rows, got {outcome:?}");
    };
    assert_eq!(batch.rows.len(), 3);
    assert!(batch.has_more);
    assert_eq!(batch.columns[0].type_name.as_deref(), Some("NUMBER(38,0)"));
    assert_eq!(
        batch.rows[0],
        vec![
            Value::Int(1),
            Value::Text("Ada".into()),
            Value::Text("2021-03-19 17:06:59.500".into()),
            Value::Bool(true),
        ]
    );
    assert_eq!(batch.rows[1][1], Value::Null);
    assert_eq!(
        batch.rows[2][2],
        Value::Text("1970-01-01 00:00:00.000".into())
    );

    let submits = requests_to(&server, "POST", "/api/v2/statements").await;
    let submit = submits.last().unwrap();
    assert_eq!(query_param(submit, "async").as_deref(), Some("true"));
    assert_eq!(body(submit)["timeout"], 30);
    let polls = requests_to(&server, "GET", STATUS_PATH).await;
    let partitions: Vec<String> = polls
        .iter()
        .filter_map(|request| query_param(request, "partition"))
        .collect();
    assert_eq!(
        partitions,
        vec!["1".to_owned()],
        "partition 2 is never needed"
    );
    assert_eq!(polls.len(), 3);
    assert!(
        polls
            .iter()
            .all(|request| header(request, "authorization").starts_with("Bearer "))
    );
}

#[tokio::test]
async fn dml_statistics_and_ddl_status_become_outcomes() {
    let server = MockServer::start().await;
    let connection = connect(&server).await;
    let mut inserted = result_set(
        vec![column("number of rows inserted", "fixed")],
        json!([["2"]]),
        1,
        1,
    );
    inserted["stats"] = json!({"numRowsInserted": 2, "numRowsUpdated": 0, "numRowsDeleted": 0});
    Mock::given(method("POST"))
        .and(body_partial_json(
            json!({"statement": "INSERT INTO t VALUES (1), (2)"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(inserted))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            json!({"statement": "CREATE TABLE t (id INT)"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(result_set(
            vec![column("status", "text")],
            json!([["Table T successfully created."]]),
            1,
            1,
        )))
        .mount(&server)
        .await;
    let sql = connection.as_sql().unwrap();
    assert_eq!(
        sql.execute("INSERT INTO t VALUES (1), (2)", &options(10))
            .await
            .unwrap(),
        StatementOutcome::Affected { rows: 2 }
    );
    assert_eq!(
        sql.execute("CREATE TABLE t (id INT)", &options(10))
            .await
            .unwrap(),
        StatementOutcome::Completed {
            message: "Table T successfully created.".into()
        }
    );
}

#[tokio::test]
async fn use_statements_apply_to_later_requests() {
    let server = MockServer::start().await;
    let connection = connect(&server).await;
    submit_answers(
        &server,
        ResponseTemplate::new(200).set_body_json(result_set(
            vec![column("status", "text")],
            json!([["Statement executed successfully."]]),
            1,
            1,
        )),
    )
    .await;
    let sql = connection.as_sql().unwrap();
    sql.execute("USE WAREHOUSE reporting_wh", &options(10))
        .await
        .unwrap();
    sql.execute("USE SCHEMA hr.staff", &options(10))
        .await
        .unwrap();
    sql.execute("SELECT 1", &options(10)).await.unwrap();
    let submits = requests_to(&server, "POST", "/api/v2/statements").await;
    assert_eq!(
        body(submits.last().unwrap()),
        json!({
            "statement": "SELECT 1",
            "timeout": 30,
            "database": "HR",
            "schema": "STAFF",
            "warehouse": "REPORTING_WH",
            "role": "ANALYST_ROLE",
        })
    );
}

#[tokio::test]
async fn transaction_control_is_rejected_without_a_request() {
    let server = MockServer::start().await;
    let connection = connect(&server).await;
    let before = server.received_requests().await.unwrap().len();
    let error = connection
        .as_sql()
        .unwrap()
        .execute("BEGIN TRANSACTION", &options(10))
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert_eq!(server.received_requests().await.unwrap().len(), before);
}

#[tokio::test]
async fn compilation_errors_report_code_and_position() {
    let server = MockServer::start().await;
    let connection = connect(&server).await;
    submit_answers(
        &server,
        ResponseTemplate::new(422).set_body_json(json!({
            "code": "001003",
            "sqlState": "42000",
            "message": "SQL compilation error:\nsyntax error line 1 at position 9 unexpected 'FORM'.",
            "statementHandle": HANDLE,
            "statementStatusUrl": STATUS_PATH,
        })),
    )
    .await;
    let error = connection
        .as_sql()
        .unwrap()
        .execute("SELECT * FORM t", &options(10))
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    assert_eq!(error.position, Some(9));
    assert!(error.detail.unwrap().contains("Snowflake error 001003"));
}

struct StatementStatus {
    cancelled: Arc<AtomicBool>,
}

impl Respond for StatementStatus {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        if self.cancelled.load(Ordering::SeqCst) {
            ResponseTemplate::new(422).set_body_json(json!({
                "code": "000604",
                "sqlState": "57014",
                "message": "SQL execution canceled",
                "statementHandle": HANDLE,
            }))
        } else {
            ResponseTemplate::new(202).set_body_json(running(HANDLE))
        }
    }
}

struct CancelStatement {
    cancelled: Arc<AtomicBool>,
}

impl Respond for CancelStatement {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        self.cancelled.store(true, Ordering::SeqCst);
        ResponseTemplate::new(200).set_body_json(json!({
            "code": "000000",
            "sqlState": "00000",
            "message": "successfully canceled",
            "statementHandle": HANDLE,
        }))
    }
}

async fn mount_cancellable_statement(server: &MockServer, submit_delay: Duration) {
    let cancelled = Arc::new(AtomicBool::new(false));
    submit_answers(
        server,
        ResponseTemplate::new(202)
            .set_body_json(running(HANDLE))
            .set_delay(submit_delay),
    )
    .await;
    Mock::given(method("GET"))
        .and(path(STATUS_PATH))
        .respond_with(StatementStatus {
            cancelled: cancelled.clone(),
        })
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path(CANCEL_PATH))
        .respond_with(CancelStatement { cancelled })
        .mount(server)
        .await;
}

#[tokio::test]
async fn cancel_reaches_the_running_statement() {
    let server = MockServer::start().await;
    let connection = connect(&server).await;
    mount_cancellable_statement(&server, Duration::ZERO).await;
    let sql = connection.as_sql().unwrap();
    let options = options(10);
    let (result, cancelled) =
        tokio::join!(sql.execute("SELECT SYSTEM$WAIT(60)", &options), async {
            while requests_to(&server, "GET", STATUS_PATH).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            sql.cancel().await
        });
    cancelled.unwrap();
    assert_eq!(result.unwrap_err().kind, ErrorKind::Cancelled);
    let cancels = requests_to(&server, "POST", CANCEL_PATH).await;
    assert_eq!(cancels.len(), 1);
    assert_eq!(header(&cancels[0], "content-length"), "0");
    assert_eq!(
        header(&cancels[0], "x-snowflake-authorization-token-type"),
        "KEYPAIR_JWT"
    );
}

#[tokio::test]
async fn cancel_before_the_handle_is_known_is_delivered_once_it_arrives() {
    let server = MockServer::start().await;
    let connection = connect(&server).await;
    mount_cancellable_statement(&server, Duration::from_millis(400)).await;
    let sql = connection.as_sql().unwrap();
    let options = options(10);
    let (result, cancelled) =
        tokio::join!(sql.execute("SELECT SYSTEM$WAIT(60)", &options), async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            sql.cancel().await
        });
    cancelled.unwrap();
    assert_eq!(result.unwrap_err().kind, ErrorKind::Cancelled);
    assert_eq!(requests_to(&server, "POST", CANCEL_PATH).await.len(), 1);
}

#[tokio::test]
async fn closed_connections_refuse_statements() {
    let server = MockServer::start().await;
    let connection = connect(&server).await;
    connection.close().await.unwrap();
    let error = connection.ping().await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Connection);
}
