/*
 * @author t0bx
 * @since 24.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use relix_core::{StatementOutcome, Value};
use relix_driver_api::{ErrorKind, ExecuteOptions};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use wiremock::matchers::{body_partial_json, method, path, query_param as query_matches};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const JOB: &str = "job_relix_1";

fn options(row_limit: usize) -> ExecuteOptions {
    ExecuteOptions { row_limit }
}

async fn queries_answer(server: &MockServer, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path(api("queries")))
        .respond_with(response)
        .mount(server)
        .await;
}

#[tokio::test]
async fn running_jobs_are_long_polled_and_paged_up_to_the_limit() {
    let server = MockServer::start().await;
    let connection = connect(
        &server,
        &[
            ("location", "EU"),
            ("dataset", "shop"),
            ("maximum_bytes_billed", "1000000000"),
        ],
    )
    .await;
    queries_answer(
        &server,
        ResponseTemplate::new(200).set_body_json(running_job(JOB)),
    )
    .await;
    let fields = json!([field("id", "INTEGER"), field("created", "TIMESTAMP")]);
    let mut first = query_response(
        JOB,
        fields.clone(),
        vec![
            row(&[json!("1"), json!("1704164645123456")]),
            row(&[json!("2"), json!(null)]),
        ],
        5,
    );
    first["pageToken"] = json!("page-2");
    Mock::given(method("GET"))
        .and(path(api(&format!("queries/{JOB}"))))
        .and(query_matches("pageToken", "page-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json({
            let mut page = query_response(
                JOB,
                fields,
                vec![
                    row(&[json!("3"), json!(null)]),
                    row(&[json!("4"), json!(null)]),
                ],
                5,
            );
            page["pageToken"] = json!("page-3");
            page
        }))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(api(&format!("queries/{JOB}"))))
        .respond_with(ResponseTemplate::new(200).set_body_json(first))
        .with_priority(2)
        .mount(&server)
        .await;

    let outcome = connection
        .as_sql()
        .unwrap()
        .execute("SELECT id, created FROM shop.orders", &options(3))
        .await
        .unwrap();
    let StatementOutcome::Rows(batch) = outcome else {
        panic!("expected rows, got {outcome:?}");
    };
    assert_eq!(batch.rows.len(), 3);
    assert!(batch.has_more);
    assert_eq!(batch.columns[0].type_name.as_deref(), Some("INT64"));
    assert_eq!(
        batch.rows[0],
        vec![
            Value::Int(1),
            Value::Text("2024-01-02 03:04:05.123456 UTC".into())
        ]
    );

    let submit = body(&requests_to(&server, "POST", &api("queries")).await[0]);
    assert_eq!(submit["query"], "SELECT id, created FROM shop.orders");
    assert_eq!(submit["useLegacySql"], false);
    assert_eq!(submit["maxResults"], 3);
    assert_eq!(submit["location"], "EU");
    assert_eq!(submit["createSession"], true);
    assert_eq!(submit["formatOptions"], json!({"useInt64Timestamp": true}));
    assert_eq!(submit["maximumBytesBilled"], "1000000000");
    assert_eq!(
        submit["defaultDataset"],
        json!({"projectId": PROJECT, "datasetId": "shop"})
    );
    assert!(
        submit["requestId"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );

    let polls = requests_to(&server, "GET", &api(&format!("queries/{JOB}"))).await;
    assert_eq!(polls.len(), 2, "one wait for completion, one further page");
    assert_eq!(query_param(&polls[0], "location").as_deref(), Some("EU"));
    assert_eq!(query_param(&polls[0], "timeoutMs").as_deref(), Some("2000"));
    assert_eq!(
        query_param(&polls[0], "formatOptions.useInt64Timestamp").as_deref(),
        Some("true")
    );
    assert_eq!(
        query_param(&polls[1], "pageToken").as_deref(),
        Some("page-2")
    );
    assert!(
        polls
            .iter()
            .all(|poll| query_param(poll, "pageToken").as_deref() != Some("page-3"))
    );
}

#[tokio::test]
async fn editor_statements_share_one_session_until_close() {
    let server = MockServer::start().await;
    let connection = connect(&server, &[]).await;
    let mut created = query_response("job_a", json!([]), vec![], 0);
    created["sessionInfo"] = json!({"sessionId": "session-42"});
    Mock::given(method("POST"))
        .and(path(api("queries")))
        .and(body_partial_json(json!({"createSession": true})))
        .respond_with(ResponseTemplate::new(200).set_body_json(created))
        .with_priority(1)
        .mount(&server)
        .await;
    queries_answer(
        &server,
        ResponseTemplate::new(200).set_body_json(query_response("job_b", json!([]), vec![], 0)),
    )
    .await;
    let sql = connection.as_sql().unwrap();
    sql.execute("BEGIN TRANSACTION", &options(10))
        .await
        .unwrap();
    sql.execute("COMMIT TRANSACTION", &options(10))
        .await
        .unwrap();
    connection.close().await.unwrap();

    let submits: Vec<serde_json::Value> = requests_to(&server, "POST", &api("queries"))
        .await
        .iter()
        .map(body)
        .collect();
    assert_eq!(submits.len(), 3);
    let session = json!([{"key": "session_id", "value": "session-42"}]);
    assert_eq!(submits[1]["query"], "COMMIT TRANSACTION");
    assert_eq!(submits[1]["connectionProperties"], session);
    assert!(submits[1].get("createSession").is_none());
    assert_eq!(submits[2]["query"], "CALL BQ.ABORT_SESSION()");
    assert_eq!(submits[2]["connectionProperties"], session);
    assert_eq!(
        connection.ping().await.unwrap_err().kind,
        ErrorKind::Connection
    );
}

#[tokio::test]
async fn dml_and_ddl_become_affected_rows_and_completion() {
    let server = MockServer::start().await;
    let connection = connect(&server, &[]).await;
    let mut dml = query_response("job_dml", json!([]), vec![], 0);
    dml["numDmlAffectedRows"] = json!("3");
    dml.as_object_mut().unwrap().remove("schema");
    Mock::given(method("POST"))
        .and(path(api("queries")))
        .and(body_partial_json(
            json!({"query": "DELETE FROM shop.orders WHERE TRUE"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(dml))
        .with_priority(1)
        .mount(&server)
        .await;
    queries_answer(
        &server,
        ResponseTemplate::new(200).set_body_json(json!({
            "kind": "bigquery#queryResponse",
            "jobReference": {"projectId": PROJECT, "jobId": "job_ddl", "location": "EU"},
            "jobComplete": true,
        })),
    )
    .await;
    let sql = connection.as_sql().unwrap();
    assert_eq!(
        sql.execute("DELETE FROM shop.orders WHERE TRUE", &options(10))
            .await
            .unwrap(),
        StatementOutcome::Affected { rows: 3 }
    );
    assert!(matches!(
        sql.execute("CREATE TABLE shop.t (id INT64)", &options(10))
            .await
            .unwrap(),
        StatementOutcome::Completed { .. }
    ));
}

#[tokio::test]
async fn query_errors_carry_reason_and_position() {
    let server = MockServer::start().await;
    let connection = connect(&server, &[]).await;
    queries_answer(
        &server,
        ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "code": 400,
                "message": "Syntax error: Expected end of input but got identifier \"FORM\" at [1:10]",
                "errors": [{"message": "Syntax error", "domain": "global", "reason": "invalidQuery", "location": "q", "locationType": "other"}],
                "status": "INVALID_ARGUMENT",
            }
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
    assert!(error.detail.unwrap().contains("invalidQuery"));
}

struct JobStatus {
    cancelled: Arc<AtomicBool>,
}

impl Respond for JobStatus {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        if self.cancelled.load(Ordering::SeqCst) {
            ResponseTemplate::new(400).set_body_json(json!({
                "error": {
                    "code": 400,
                    "message": "Job execution was cancelled: User requested cancellation",
                    "errors": [{"message": "Job execution was cancelled", "domain": "global", "reason": "stopped"}],
                    "status": "INVALID_ARGUMENT",
                }
            }))
        } else {
            ResponseTemplate::new(200)
                .set_body_json(running_job(JOB))
                .set_delay(Duration::from_millis(50))
        }
    }
}

struct CancelJob {
    cancelled: Arc<AtomicBool>,
}

impl Respond for CancelJob {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        self.cancelled.store(true, Ordering::SeqCst);
        ResponseTemplate::new(200).set_body_json(json!({
            "kind": "bigquery#jobCancelResponse",
            "job": {"jobReference": {"projectId": PROJECT, "jobId": JOB, "location": "EU"}, "status": {"state": "RUNNING"}},
        }))
    }
}

async fn mount_cancellable_job(server: &MockServer, submit_delay: Duration) -> String {
    let cancelled = Arc::new(AtomicBool::new(false));
    queries_answer(
        server,
        ResponseTemplate::new(200)
            .set_body_json(running_job(JOB))
            .set_delay(submit_delay),
    )
    .await;
    Mock::given(method("GET"))
        .and(path(api(&format!("queries/{JOB}"))))
        .respond_with(JobStatus {
            cancelled: cancelled.clone(),
        })
        .mount(server)
        .await;
    let cancel_path = api(&format!("jobs/{JOB}/cancel"));
    Mock::given(method("POST"))
        .and(path(cancel_path.clone()))
        .respond_with(CancelJob { cancelled })
        .mount(server)
        .await;
    cancel_path
}

#[tokio::test]
async fn cancel_reaches_the_running_job() {
    let server = MockServer::start().await;
    let connection = connect(&server, &[]).await;
    let cancel_path = mount_cancellable_job(&server, Duration::ZERO).await;
    let sql = connection.as_sql().unwrap();
    let options = options(10);
    let (result, cancelled) = tokio::join!(sql.execute("SELECT 1", &options), async {
        let status = api(&format!("queries/{JOB}"));
        while requests_to(&server, "GET", &status).await.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        sql.cancel().await
    });
    cancelled.unwrap();
    assert_eq!(result.unwrap_err().kind, ErrorKind::Cancelled);
    let cancels = requests_to(&server, "POST", &cancel_path).await;
    assert_eq!(cancels.len(), 1);
    assert_eq!(query_param(&cancels[0], "location").as_deref(), Some("EU"));
    assert_eq!(header(&cancels[0], "content-length"), "0");
    assert_eq!(
        header(&cancels[0], "authorization"),
        format!("Bearer {ACCESS_TOKEN}")
    );
}

#[tokio::test]
async fn cancel_before_the_job_id_is_known_is_delivered_afterwards() {
    let server = MockServer::start().await;
    let connection = connect(&server, &[]).await;
    let cancel_path = mount_cancellable_job(&server, Duration::from_millis(400)).await;
    let sql = connection.as_sql().unwrap();
    let options = options(10);
    let (result, cancelled) = tokio::join!(sql.execute("SELECT 1", &options), async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        sql.cancel().await
    });
    cancelled.unwrap();
    assert_eq!(result.unwrap_err().kind, ErrorKind::Cancelled);
    assert_eq!(requests_to(&server, "POST", &cancel_path).await.len(), 1);
}
