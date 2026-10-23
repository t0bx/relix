/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{connect, connect_with, console, database_name, documents, mongodb_config};
use relix_core::{
    CollectionKind, CollectionRef, DocumentQuery, DocumentWrite, RowBatch, Secret,
    StatementOutcome, Value,
};
use relix_driver_api::{DatabaseDriver, ErrorKind, ExecuteOptions, QueryExecutor};
use relix_driver_mongodb::MongoDbDriver;
use relix_testing::integration::unique_name;
use serde_json::json;
use std::time::{Duration, Instant};

async fn run(executor: &dyn QueryExecutor, statement: &str) -> StatementOutcome {
    executor
        .execute(statement, &ExecuteOptions::default())
        .await
        .unwrap_or_else(|error| panic!("{statement}: {error}"))
}

fn rows(outcome: StatementOutcome) -> RowBatch {
    match outcome {
        StatementOutcome::Rows(batch) => batch,
        other => panic!("expected rows, got {other:?}"),
    }
}

async fn drop_collection(executor: &dyn QueryExecutor, name: &str) {
    let _ = executor
        .execute(
            &format!(r#"{{"drop": "{name}"}}"#),
            &ExecuteOptions::default(),
        )
        .await;
}

async fn seed(executor: &dyn QueryExecutor, collection: &str, count: usize) {
    let documents: Vec<_> = (0..count)
        .map(|index| json!({"n": index, "group": index % 3, "label": format!("doc {index}")}))
        .collect();
    let command = json!({"insert": collection, "documents": documents}).to_string();
    let outcome = run(executor, &command).await;
    assert_eq!(outcome, StatementOutcome::Affected { rows: count as u64 });
}

#[tokio::test]
async fn connects_pings_and_closes() {
    let Some(connection) = connect().await else {
        return;
    };
    let info = connection.server_info();
    assert_eq!(info.product, "MongoDB");
    assert!(info.version.starts_with('8'), "{}", info.version);
    connection.ping().await.unwrap();
    assert!(connection.as_sql().is_none());
    assert!(connection.as_document().is_some());
    connection.close().await.unwrap();
    let error = connection.ping().await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Connection);
}

#[tokio::test]
async fn wrong_passwords_are_authentication_errors() {
    let Some(mut config) = mongodb_config() else {
        return;
    };
    config.password = Some(Secret::new("definitely-wrong"));
    let error = MongoDbDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication, "{error}");
    assert!(!error.to_string().contains("definitely-wrong"));
}

#[tokio::test]
async fn unreachable_servers_fail_within_the_connect_timeout() {
    let Some(mut config) = mongodb_config() else {
        return;
    };
    config.settings.port = Some(1);
    config.settings.connect_timeout_secs = 2;
    let started = Instant::now();
    let error = MongoDbDriver::new().connect(&config).await.err().unwrap();
    assert!(
        matches!(error.kind, ErrorKind::Connection | ErrorKind::Timeout),
        "{error}"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn connection_strings_are_supported() {
    let Some(mut config) = mongodb_config() else {
        return;
    };
    let host = config.host_or_default().to_owned();
    let port = config.settings.port.unwrap();
    config.settings.properties.insert(
        "uri".into(),
        format!("mongodb://relix@{host}:{port}/?authSource=admin&directConnection=true"),
    );
    config.settings.host = Some("ignored.invalid".into());
    let connection = connect_with(&config).await;
    connection.ping().await.unwrap();
}

#[tokio::test]
async fn browses_databases_collections_views_and_time_series() {
    let Some(connection) = connect().await else {
        return;
    };
    let executor = console(connection.as_ref());
    let plain = unique_name("browse");
    let view = format!("{plain}_view");
    let series = format!("{plain}_series");
    seed(executor, &plain, 5).await;
    run(
        executor,
        &json!({"create": view, "viewOn": plain, "pipeline": [{"$match": {"group": 0}}]})
            .to_string(),
    )
    .await;
    run(
        executor,
        &json!({"create": series, "timeseries": {"timeField": "ts", "metaField": "sensor"}})
            .to_string(),
    )
    .await;

    let documents = documents(connection.as_ref());
    let namespaces = documents.namespaces().await.unwrap();
    let default = namespaces.iter().find(|ns| ns.is_default).unwrap();
    assert_eq!(default.name, database_name());
    assert!(namespaces.iter().any(|ns| ns.name == "admin"));

    let collections = documents.collections(&database_name()).await.unwrap();
    let find = |name: &str| {
        collections
            .iter()
            .find(|info| info.collection.name == name)
            .unwrap_or_else(|| panic!("{name} missing"))
    };
    assert_eq!(find(&plain).kind, CollectionKind::Collection);
    assert_eq!(find(&plain).document_count, Some(5));
    assert_eq!(find(&view).kind, CollectionKind::View);
    assert_eq!(
        find(&view).detail.as_deref(),
        Some(format!("view on {plain}").as_str())
    );
    assert_eq!(find(&series).kind, CollectionKind::TimeSeries);

    for name in [&view, &series, &plain] {
        drop_collection(executor, name).await;
    }
}

#[tokio::test]
async fn describes_indexes_validators_and_statistics() {
    let Some(connection) = connect().await else {
        return;
    };
    let executor = console(connection.as_ref());
    let name = unique_name("describe");
    run(
        executor,
        &json!({
            "create": name,
            "validator": {"$jsonSchema": {"bsonType": "object", "required": ["email"]}}
        })
        .to_string(),
    )
    .await;
    let documents = documents(connection.as_ref());
    let collection = CollectionRef::new(database_name(), &name);
    let created = documents
        .write(
            &collection,
            &DocumentWrite::CreateIndex {
                definition: json!({"keys": [["email", 1], ["created", -1]], "options": {"unique": true}}),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        created.message.as_deref(),
        Some("created index email_1_created_-1")
    );
    documents
        .write(
            &collection,
            &DocumentWrite::CreateIndex {
                definition: json!({"keys": {"created": 1}, "options": {"name": "expiry", "expireAfterSeconds": 3600}}),
            },
        )
        .await
        .unwrap();
    documents
        .write(
            &collection,
            &DocumentWrite::Insert {
                document: json!({"email": "a@example.com", "created": {"$date": "2026-01-01T00:00:00Z"}}),
            },
        )
        .await
        .unwrap();

    let details = documents.describe_collection(&collection).await.unwrap();
    assert_eq!(details.key_fields, vec!["_id"]);
    assert_eq!(
        details.schema,
        Some(json!({"bsonType": "object", "required": ["email"]}))
    );
    let compound = details
        .indexes
        .iter()
        .find(|index| index.name == "email_1_created_-1")
        .unwrap();
    assert!(compound.unique);
    let ttl = details
        .indexes
        .iter()
        .find(|index| index.name == "expiry")
        .unwrap();
    assert_eq!(ttl.detail.as_deref(), Some("TTL 3600 s"));
    assert!(details.indexes.iter().any(|index| index.name == "_id_"));
    assert!(
        details
            .properties
            .iter()
            .any(|(name, value)| name == "documents" && value == "1"),
        "{:?}",
        details.properties
    );

    documents
        .write(
            &collection,
            &DocumentWrite::DropIndex {
                name: "expiry".into(),
            },
        )
        .await
        .unwrap();
    let details = documents.describe_collection(&collection).await.unwrap();
    assert!(!details.indexes.iter().any(|index| index.name == "expiry"));
    let rejected = documents
        .write(
            &collection,
            &DocumentWrite::Insert {
                document: json!({"no_email": true}),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(rejected.kind, ErrorKind::Query);
    drop_collection(executor, &name).await;
}

#[tokio::test]
async fn finds_pages_with_filters_sorting_and_projection() {
    let Some(connection) = connect().await else {
        return;
    };
    let executor = console(connection.as_ref());
    let name = unique_name("find");
    seed(executor, &name, 25).await;
    let documents = documents(connection.as_ref());
    let collection = CollectionRef::new(database_name(), &name);

    let mut query = DocumentQuery::first_page(10);
    query.sort = Some(json!([["n", -1]]));
    let first = documents.find(&collection, &query).await.unwrap();
    assert_eq!(first.documents.len(), 10);
    assert_eq!(first.total, Some(25));
    assert_eq!(first.documents[0].body["n"], json!(24));
    assert!(first.documents[0].key.get("$oid").is_some());
    query.page_token = first.next_page.clone();
    assert_eq!(query.page_token.as_deref(), Some("10"));
    let second = documents.find(&collection, &query).await.unwrap();
    assert_eq!(second.documents[0].body["n"], json!(14));
    query.page_token = Some("20".into());
    let last = documents.find(&collection, &query).await.unwrap();
    assert_eq!(last.documents.len(), 5);
    assert_eq!(last.next_page, None);

    let filtered = DocumentQuery {
        filter: Some(json!({"group": 0, "n": {"$gte": 10}})),
        projection: Some(json!({"label": 0})),
        sort: Some(json!({"n": 1})),
        ..DocumentQuery::first_page(50)
    };
    let page = documents.find(&collection, &filtered).await.unwrap();
    let numbers: Vec<_> = page
        .documents
        .iter()
        .map(|doc| doc.body["n"].clone())
        .collect();
    assert_eq!(
        numbers,
        vec![json!(12), json!(15), json!(18), json!(21), json!(24)]
    );
    assert_eq!(page.total, Some(5));
    assert!(page.documents[0].body.get("label").is_none());

    let invalid = DocumentQuery {
        filter: Some(json!({"n": {"$bogus": 1}})),
        ..DocumentQuery::first_page(5)
    };
    let error = documents.find(&collection, &invalid).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    drop_collection(executor, &name).await;
}

#[tokio::test]
async fn writes_round_trip_extended_json_values() {
    let Some(connection) = connect().await else {
        return;
    };
    let name = unique_name("writes");
    let documents = documents(connection.as_ref());
    let collection = CollectionRef::new(database_name(), &name);
    let original = json!({
        "count": {"$numberLong": "7"},
        "ratio": 2.0,
        "when": {"$date": "2026-10-23T08:30:00Z"},
        "blob": {"$binary": {"base64": "AQID", "subType": "00"}},
        "price": {"$numberDecimal": "19.99"},
        "tags": ["a", "b"],
    });
    let inserted = documents
        .write(
            &collection,
            &DocumentWrite::Insert {
                document: original.clone(),
            },
        )
        .await
        .unwrap();
    let key = inserted.key.unwrap();
    assert!(key.get("$oid").is_some());

    let page = documents
        .find(&collection, &DocumentQuery::first_page(10))
        .await
        .unwrap();
    let stored = &page.documents[0];
    assert_eq!(stored.key, key);
    for field in ["count", "ratio", "when", "blob", "price", "tags"] {
        assert_eq!(stored.body[field], original[field], "field {field}");
    }

    let mut edited = stored.body.clone();
    edited["tags"] = json!(["c"]);
    let replaced = documents
        .write(
            &collection,
            &DocumentWrite::Replace {
                key: key.clone(),
                document: edited,
            },
        )
        .await
        .unwrap();
    assert_eq!(replaced.affected, 1);
    let executor = console(connection.as_ref());
    let typed = rows(
        run(
            executor,
            &json!({"aggregate": name, "pipeline": [{"$project": {"_id": 0, "t": {"$type": "$count"}, "tags": 1}}]})
                .to_string(),
        )
        .await,
    );
    assert_eq!(
        typed.rows[0][typed.column_index("t").unwrap()],
        Value::Text("long".into())
    );
    assert_eq!(
        typed.rows[0][typed.column_index("tags").unwrap()],
        Value::Text("[\"c\"]".into())
    );

    let deleted = documents
        .write(&collection, &DocumentWrite::Delete { key: key.clone() })
        .await
        .unwrap();
    assert_eq!(deleted.affected, 1);
    let missing = documents
        .write(&collection, &DocumentWrite::Delete { key: key.clone() })
        .await
        .unwrap_err();
    assert_eq!(missing.kind, ErrorKind::ConcurrentModification);
    let gone = documents
        .write(
            &collection,
            &DocumentWrite::Replace {
                key,
                document: json!({"x": 1}),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(gone.kind, ErrorKind::ConcurrentModification);
    drop_collection(executor, &name).await;
}

#[tokio::test]
async fn console_runs_commands_with_cursors_limits_and_databases() {
    let Some(connection) = connect().await else {
        return;
    };
    let executor = console(connection.as_ref());
    let name = unique_name("console");
    seed(executor, &name, 30).await;

    let limited = executor
        .execute(
            &format!(r#"{{"find": "{name}", "filter": {{"n": {{"$gte": 5}}}}, "sort": {{"n": 1}}, "batchSize": 3}}"#),
            &ExecuteOptions { row_limit: 7 },
        )
        .await
        .unwrap();
    let batch = rows(limited);
    assert_eq!(batch.rows.len(), 7);
    assert!(batch.has_more);
    assert_eq!(batch.columns[0].name, "_id");
    assert_eq!(
        batch.rows[0][batch.column_index("n").unwrap()],
        Value::Int(5)
    );

    let everything = rows(run(executor, &format!(r#"{{"find": "{name}"}}"#)).await);
    assert_eq!(everything.rows.len(), 30);
    assert!(!everything.has_more);

    let grouped = rows(
        run(
            executor,
            &format!(
                r#"{{"aggregate": "{name}", "pipeline": [{{"$group": {{"_id": "$group", "total": {{"$sum": 1}}}}}}, {{"$sort": {{"_id": 1}}}}]}}"#
            ),
        )
        .await,
    );
    assert_eq!(grouped.rows.len(), 3);
    assert_eq!(grouped.rows[0], vec![Value::Int(0), Value::Int(10)]);

    let updated = run(
        executor,
        &format!(
            r#"{{"update": "{name}", "updates": [{{"q": {{"group": 1}}, "u": {{"$set": {{"flag": true}}}}, "multi": true}}]}}"#
        ),
    )
    .await;
    assert_eq!(updated, StatementOutcome::Affected { rows: 10 });

    let count = rows(
        run(
            executor,
            &format!(r#"{{"count": "{name}", "query": {{"flag": true}}}};"#),
        )
        .await,
    );
    assert_eq!(
        count.rows[0][count.column_index("n").unwrap()],
        Value::Int(10)
    );

    let duplicate = executor
        .execute(
            &format!(r#"{{"insert": "{name}", "documents": [{{"_id": 1}}, {{"_id": 1}}]}}"#),
            &ExecuteOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(duplicate.message.contains("E11000"), "{duplicate}");

    let other = unique_name("other_db");
    let switched = run(executor, &format!("use {other}")).await;
    assert!(matches!(switched, StatementOutcome::Completed { .. }));
    run(executor, r#"{"insert": "things", "documents": [{"a": 1}]}"#).await;
    let elsewhere = rows(run(executor, r#"{"find": "things"}"#).await);
    assert_eq!(elsewhere.rows.len(), 1);
    let admin = rows(
        run(
            executor,
            r#"{"listDatabases": 1, "nameOnly": true, "$db": "admin"}"#,
        )
        .await,
    );
    assert!(admin.column_index("databases").is_some());
    run(executor, r#"{"dropDatabase": 1}"#).await;
    run(executor, &format!("use {}", database_name())).await;

    let syntax = executor
        .execute("{\"find\": \"x\",, }", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(syntax.kind, ErrorKind::Query);
    assert_eq!(syntax.position, Some(13));
    let unknown = executor
        .execute(r#"{"noSuchCommand": 1}"#, &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(unknown.kind, ErrorKind::Query);
    drop_collection(executor, &name).await;
}

#[tokio::test]
async fn cancel_kills_the_running_server_operation() {
    let Some(connection) = connect().await else {
        return;
    };
    let watcher = connect().await.unwrap();
    let executor = console(connection.as_ref());
    let name = unique_name("cancel");
    seed(executor, &name, 50).await;
    let slow = format!(
        r#"{{"find": "{name}", "filter": {{"$where": "sleep(200) || true"}}, "batchSize": 1000}}"#
    );
    let options = ExecuteOptions::default();
    let started = Instant::now();
    let (outcome, cancelled) = tokio::join!(executor.execute(&slow, &options), async {
        tokio::time::sleep(Duration::from_millis(700)).await;
        executor.cancel().await
    });
    cancelled.unwrap();
    let error = outcome.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(5));

    tokio::time::sleep(Duration::from_millis(500)).await;
    let still_running = rows(
        run(
            console(watcher.as_ref()),
            &format!(
                r#"{{"aggregate": 1, "pipeline": [{{"$currentOp": {{}}}}, {{"$match": {{"ns": "{}.{name}"}}}}], "$db": "admin"}}"#,
                database_name()
            ),
        )
        .await,
    );
    assert!(still_running.rows.is_empty(), "{still_running:?}");
    executor.cancel().await.unwrap();
    drop_collection(executor, &name).await;
}
