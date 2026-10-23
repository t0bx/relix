/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Product, connect, connect_with, drop_quietly, row_value, rows, run, try_run};
use relix_core::{StatementOutcome, Value};
use relix_driver_api::{DatabaseConnection, ErrorKind, ExecuteOptions};
use relix_testing::integration::unique_name;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn column_names(batch: &relix_core::RowBatch) -> Vec<&str> {
    batch
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect()
}

async fn console_renders_each_kind_of_response(product: Product) {
    let Some(connection) = connect(product).await else {
        return;
    };
    let connection = connection.as_ref();
    let index = unique_name("relix_console");
    let created = run(
        connection,
        &format!(
            "# create an index\nPUT /{index}\n{{\n  \"settings\": {{\"number_of_replicas\": 0}},\n  \
             \"mappings\": {{\"properties\": {{\"tag\": {{\"type\": \"keyword\"}}}}}}\n}}"
        ),
    )
    .await;
    assert_eq!(
        created,
        StatementOutcome::Completed {
            message: format!("acknowledged (index {index})")
        }
    );
    let bulk = rows(
        run(
            connection,
            &format!(
                "POST /_bulk?refresh=true\n{{\"index\": {{\"_index\": \"{index}\", \"_id\": \"1\"}}}}\n\
                 {{\"tag\": \"a\", \"meta\": {{\"x\": 1}}}}\n{{\"create\": {{\"_index\": \"{index}\", \"_id\": \"1\"}}}}\n\
                 {{\"tag\": \"dup\"}}\n{{\"index\": {{\"_index\": \"{index}\", \"_id\": \"2\"}}}}\n{{\"tag\": \"b\"}}"
            ),
        )
        .await,
    );
    assert_eq!(bulk.rows.len(), 3);
    assert_eq!(bulk.rows[1][3], Value::Int(409));
    assert!(!bulk.rows[1][5].is_null());

    let hits = rows(
        run(
            connection,
            &format!("POST /{index}/_search\n{{\"sort\": [\"tag\"]}}"),
        )
        .await,
    );
    assert_eq!(&column_names(&hits)[..3], ["_index", "_id", "_score"]);
    assert!(column_names(&hits).contains(&"tag"));
    assert_eq!(hits.rows.len(), 2);
    let meta = hits.column_index("meta").unwrap();
    assert_eq!(hits.rows[0][meta], Value::Text("{\"x\":1}".into()));
    assert!(!hits.has_more);

    let limited = console_with_limit(connection, &format!("GET /{index}/_search"), 1).await;
    assert_eq!(rows(limited.clone()).rows.len(), 1);
    assert!(rows(limited).has_more);

    let aggregations = rows(
        run(
            connection,
            &format!(
                "POST /{index}/_search\n{{\"size\": 0, \"aggs\": {{\"tags\": {{\"terms\": {{\"field\": \"tag\"}}}}}}}}"
            ),
        )
        .await,
    );
    assert_eq!(column_names(&aggregations), ["key", "value"]);
    let Some(Value::Text(buckets)) = row_value(&aggregations, "tags") else {
        panic!("missing aggregation");
    };
    assert!(buckets.contains("\"doc_count\":1"));

    let health = rows(run(connection, "GET _cluster/health").await);
    assert!(row_value(&health, "status").is_some());
    let cat = rows(
        run(
            connection,
            &format!("GET /_cat/indices/{index}?format=json"),
        )
        .await,
    );
    assert!(column_names(&cat).contains(&"health"));
    let text = rows(run(connection, &format!("GET /_cat/indices/{index}?v")).await);
    assert_eq!(column_names(&text), ["response"]);
    assert_eq!(text.rows.len(), 2);

    let document = run(
        connection,
        &format!("PUT /{index}/_doc/3?refresh=true\n{{\"tag\": \"c\"}}"),
    )
    .await;
    assert_eq!(
        document,
        StatementOutcome::Completed {
            message: format!("created {index}/3")
        }
    );
    let deleted = run(
        connection,
        &format!("POST /{index}/_delete_by_query?refresh=true\n{{\"query\": {{\"term\": {{\"tag\": \"c\"}}}}}}"),
    )
    .await;
    assert_eq!(deleted, StatementOutcome::Affected { rows: 1 });
    assert_eq!(
        run(connection, &format!("HEAD /{index}_missing")).await,
        StatementOutcome::Completed {
            message: "HTTP 404 Not Found".into()
        }
    );
    drop_quietly(connection, &index).await;
}
for_each_product!(console_renders_each_kind_of_response);

async fn console_with_limit(
    connection: &dyn DatabaseConnection,
    request: &str,
    row_limit: usize,
) -> StatementOutcome {
    connection
        .as_query()
        .unwrap()
        .execute(request, &ExecuteOptions { row_limit })
        .await
        .unwrap()
}

async fn console_errors_are_typed_and_positioned(product: Product) {
    let Some(connection) = connect(product).await else {
        return;
    };
    let connection = connection.as_ref();
    let statement = "POST /_search\n{\"query\": {\"match_all\": {}}";
    let syntax = try_run(connection, statement).await.unwrap_err();
    assert_eq!(syntax.kind, ErrorKind::Query);
    assert!(syntax.position.unwrap() >= 14);
    let unknown = try_run(connection, "POST /_search\n{\"query\": {\"mach\": {}}}")
        .await
        .unwrap_err();
    assert_eq!(unknown.kind, ErrorKind::Query);
    assert!(unknown.message.contains("mach"), "{}", unknown.message);
    let missing = try_run(connection, "GET /relix_no_such_index_x/_search")
        .await
        .unwrap_err();
    assert_eq!(missing.kind, ErrorKind::Query);
    assert!(
        missing.message.contains("no such index"),
        "{}",
        missing.message
    );
    let outside = try_run(connection, "GET http://example.com/")
        .await
        .unwrap_err();
    assert_eq!(outside.kind, ErrorKind::Query);
}
for_each_product!(console_errors_are_typed_and_positioned);

async fn cancel_stops_the_request_and_its_server_tasks(product: Product) {
    let Some(connection) = connect(product).await else {
        return;
    };
    let connection: Arc<Box<dyn DatabaseConnection>> = Arc::new(connection);
    let source = unique_name("relix_cancel_src");
    let target = unique_name("relix_cancel_dst");
    let mut bulk = format!("POST /{source}/_bulk?refresh=true\n");
    for n in 0..40 {
        bulk.push_str(&format!("{{\"index\": {{}}}}\n{{\"n\": {n}}}\n"));
    }
    run(connection.as_ref().as_ref(), &bulk).await;
    let running = connection.clone();
    let reindex = format!(
        "POST /_reindex?requests_per_second=1\n{{\"source\": {{\"index\": \"{source}\", \"size\": 1}}, \
         \"dest\": {{\"index\": \"{target}\"}}}}"
    );
    let started = Instant::now();
    let task = tokio::spawn(async move {
        running
            .as_query()
            .unwrap()
            .execute(&reindex, &ExecuteOptions::default())
            .await
    });
    tokio::time::sleep(Duration::from_millis(1500)).await;
    connection.as_query().unwrap().cancel().await.unwrap();
    let error = task.await.unwrap().unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(10));
    let mut still_running = true;
    for _ in 0..20 {
        let tasks = rows(
            run(
                connection.as_ref().as_ref(),
                "GET /_tasks?actions=*reindex&detailed=true",
            )
            .await,
        );
        let Some(Value::Text(nodes)) = row_value(&tasks, "nodes") else {
            panic!("no nodes in tasks response");
        };
        if !nodes.contains("reindex") {
            still_running = false;
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(!still_running, "the reindex task kept running");
    drop_quietly(connection.as_ref().as_ref(), &source).await;
    drop_quietly(connection.as_ref().as_ref(), &target).await;
}
for_each_product!(cancel_stops_the_request_and_its_server_tasks);

async fn query_timeouts_are_reported(product: Product) {
    let Some(mut config) = common::config(product) else {
        return;
    };
    config.settings.query_timeout_secs = Some(1);
    let connection = connect_with(product, &config).await;
    let started = Instant::now();
    let error = try_run(
        connection.as_ref(),
        "GET /_cluster/health?wait_for_nodes=99&timeout=20s",
    )
    .await
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert!(started.elapsed() < Duration::from_secs(5));
}
for_each_product!(query_timeouts_are_reported);
