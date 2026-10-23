/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use aws_sdk_dynamodb::types::{
    AttributeDefinition, BillingMode, GlobalSecondaryIndex, KeySchemaElement, KeyType,
    LocalSecondaryIndex, Projection, ProjectionType, ScalarAttributeType, StreamSpecification,
    StreamViewType,
};
use common::{admin_client, connect, connect_with, console, documents, dynamodb_config, region};
use relix_core::{
    CollectionKind, CollectionRef, DocumentIndex, DocumentQuery, DocumentWrite, RowBatch,
    StatementOutcome, Value,
};
use relix_driver_api::{
    DatabaseConnection, DatabaseDriver, DocumentConnection, ErrorKind, ExecuteOptions,
};
use relix_driver_dynamodb::DynamoDbDriver;
use relix_testing::integration::unique_name;
use serde_json::{Value as Json, json};
use std::time::Duration;

fn key(name: &str, kind: KeyType) -> KeySchemaElement {
    KeySchemaElement::builder()
        .attribute_name(name)
        .key_type(kind)
        .build()
        .unwrap()
}

fn attribute(name: &str, kind: ScalarAttributeType) -> AttributeDefinition {
    AttributeDefinition::builder()
        .attribute_name(name)
        .attribute_type(kind)
        .build()
        .unwrap()
}

fn projection(kind: ProjectionType) -> Projection {
    Projection::builder().projection_type(kind).build()
}

async fn create_table(name: &str) {
    admin_client()
        .create_table()
        .table_name(name)
        .billing_mode(BillingMode::PayPerRequest)
        .key_schema(key("pk", KeyType::Hash))
        .key_schema(key("sk", KeyType::Range))
        .attribute_definitions(attribute("pk", ScalarAttributeType::S))
        .attribute_definitions(attribute("sk", ScalarAttributeType::N))
        .attribute_definitions(attribute("email", ScalarAttributeType::S))
        .attribute_definitions(attribute("score", ScalarAttributeType::N))
        .global_secondary_indexes(
            GlobalSecondaryIndex::builder()
                .index_name("byEmail")
                .key_schema(key("email", KeyType::Hash))
                .projection(projection(ProjectionType::All))
                .build()
                .unwrap(),
        )
        .local_secondary_indexes(
            LocalSecondaryIndex::builder()
                .index_name("byScore")
                .key_schema(key("pk", KeyType::Hash))
                .key_schema(key("score", KeyType::Range))
                .projection(projection(ProjectionType::KeysOnly))
                .build()
                .unwrap(),
        )
        .stream_specification(
            StreamSpecification::builder()
                .stream_enabled(true)
                .stream_view_type(StreamViewType::NewImage)
                .build()
                .unwrap(),
        )
        .send()
        .await
        .unwrap();
}

async fn delete_table(name: &str) {
    let _ = admin_client().delete_table().table_name(name).send().await;
}

async fn seed(documents: &dyn DocumentConnection, table: &CollectionRef, count: usize) {
    for index in 0..count {
        documents
            .write(
                table,
                &DocumentWrite::Insert {
                    document: json!({
                        "pk": format!("user#{}", index % 3),
                        "sk": index,
                        "email": format!("u{index}@example.com"),
                        "score": index as f64 * 1.5,
                        "label": format!("item {index}"),
                    }),
                },
            )
            .await
            .unwrap();
    }
}

fn rows(outcome: StatementOutcome) -> RowBatch {
    match outcome {
        StatementOutcome::Rows(batch) => batch,
        other => panic!("expected rows, got {other:?}"),
    }
}

fn numbers(documents: &[relix_core::Document], field: &str) -> Vec<Json> {
    documents
        .iter()
        .map(|document| document.body[field].clone())
        .collect()
}

async fn wait_for_active_index(
    documents: &dyn DocumentConnection,
    table: &CollectionRef,
    name: &str,
) -> DocumentIndex {
    for _ in 0..50 {
        let details = documents.describe_collection(table).await.unwrap();
        let index = details
            .indexes
            .into_iter()
            .find(|index| index.name == name)
            .unwrap();
        if index
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("active")
        {
            return index;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("index {name} did not become active");
}

async fn fixture(prefix: &str) -> Option<(Box<dyn DatabaseConnection>, CollectionRef)> {
    let connection = connect().await?;
    let name = unique_name(prefix);
    create_table(&name).await;
    Some((connection, CollectionRef::new(region(), name)))
}

#[tokio::test]
async fn connects_pings_and_closes() {
    let Some(connection) = connect().await else {
        return;
    };
    let info = connection.server_info();
    assert!(info.product.starts_with("DynamoDB ("), "{}", info.product);
    assert_eq!(info.version, "2012-08-10");
    connection.ping().await.unwrap();
    assert!(connection.as_sql().is_none());
    connection.close().await.unwrap();
    assert_eq!(
        connection.ping().await.unwrap_err().kind,
        ErrorKind::Connection
    );
}

#[tokio::test]
async fn unreachable_endpoints_are_connection_errors() {
    let Some(mut config) = dynamodb_config() else {
        return;
    };
    config
        .settings
        .properties
        .insert("endpoint_url".into(), "http://127.0.0.1:1".into());
    config.settings.connect_timeout_secs = 3;
    let error = DynamoDbDriver::new().connect(&config).await.err().unwrap();
    assert!(
        matches!(error.kind, ErrorKind::Connection | ErrorKind::Timeout),
        "{error}"
    );
    assert!(!error.to_string().contains("relix_test"));
}

#[tokio::test]
async fn the_standard_credential_chain_is_used_without_access_keys() {
    let Some(mut config) = dynamodb_config() else {
        return;
    };
    if std::env::var("AWS_ACCESS_KEY_ID").is_err() {
        eprintln!("skipping: export AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY to test the chain");
        return;
    }
    config.settings.username = None;
    config.password = None;
    let connection = connect_with(&config).await;
    connection.ping().await.unwrap();
}

#[tokio::test]
async fn browses_the_region_and_describes_tables() {
    let Some((connection, table)) = fixture("describe").await else {
        return;
    };
    let documents = documents(connection.as_ref());
    seed(documents, &table, 4).await;
    let namespaces = documents.namespaces().await.unwrap();
    assert_eq!(namespaces.len(), 1);
    assert_eq!(namespaces[0].name, region());
    assert!(namespaces[0].is_default);

    let tables = documents.collections(&region()).await.unwrap();
    let listed = tables.iter().find(|info| info.collection == table).unwrap();
    assert_eq!(listed.kind, CollectionKind::Table);
    assert_eq!(listed.detail.as_deref(), Some("active"));
    assert_eq!(listed.document_count, Some(4));

    let details = documents.describe_collection(&table).await.unwrap();
    assert_eq!(details.key_fields, vec!["pk", "sk"]);
    let schema = details.schema.unwrap();
    assert_eq!(
        schema["KeySchema"],
        json!([{"AttributeName": "pk", "KeyType": "HASH"}, {"AttributeName": "sk", "KeyType": "RANGE"}])
    );
    let global = details
        .indexes
        .iter()
        .find(|index| index.name == "byEmail")
        .unwrap();
    assert_eq!(
        global.detail.as_deref(),
        Some("global, active, ALL projection")
    );
    let local = details
        .indexes
        .iter()
        .find(|index| index.name == "byScore")
        .unwrap();
    assert_eq!(local.detail.as_deref(), Some("local, KEYS_ONLY projection"));
    let property = |name: &str| {
        details
            .properties
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(property("billing mode").as_deref(), Some("PAY_PER_REQUEST"));
    assert!(property("stream").unwrap().starts_with("NEW_IMAGE"));
    delete_table(&table.name).await;
}

#[tokio::test]
async fn scans_and_queries_page_through_items() {
    let Some((connection, table)) = fixture("find").await else {
        return;
    };
    let documents = documents(connection.as_ref());
    seed(documents, &table, 12).await;

    let mut scan = DocumentQuery::first_page(5);
    let mut seen = Vec::new();
    loop {
        let page = documents.find(&table, &scan).await.unwrap();
        assert!(page.documents.len() <= 5);
        seen.extend(numbers(&page.documents, "sk"));
        match page.next_page {
            Some(token) => scan.page_token = Some(token),
            None => break,
        }
    }
    seen.sort_by_key(|value| value.as_i64());
    assert_eq!(seen, (0..12).map(|n| json!(n)).collect::<Vec<_>>());

    let query = DocumentQuery {
        filter: Some(json!({
            "KeyConditionExpression": "pk = :pk AND sk >= :from",
            "FilterExpression": "#l <> :skip",
            "ExpressionAttributeNames": {"#l": "label"},
            "ExpressionAttributeValues": {":pk": {"S": "user#1"}, ":from": 4, ":skip": "item 7"}
        })),
        sort: Some(json!("desc")),
        projection: Some(json!(["label"])),
        ..DocumentQuery::first_page(10)
    };
    let page = documents.find(&table, &query).await.unwrap();
    assert_eq!(numbers(&page.documents, "sk"), vec![json!(10), json!(4)]);
    assert_eq!(page.documents[0].key, json!({"pk": "user#1", "sk": 10}));
    assert!(page.documents[0].body.get("email").is_none());
    assert_eq!(page.documents[0].body["label"], json!("item 10"));

    let by_email = DocumentQuery {
        filter: Some(json!({
            "IndexName": "byEmail",
            "KeyConditionExpression": "email = :e",
            "ExpressionAttributeValues": {":e": "u5@example.com"}
        })),
        ..DocumentQuery::first_page(10)
    };
    let page = documents.find(&table, &by_email).await.unwrap();
    assert_eq!(numbers(&page.documents, "sk"), vec![json!(5)]);
    assert_eq!(page.documents[0].body["score"], json!(7.5));

    let selective = DocumentQuery {
        filter: Some(json!({
            "FilterExpression": "sk = :n",
            "ExpressionAttributeValues": {":n": 11}
        })),
        ..DocumentQuery::first_page(2)
    };
    let page = documents.find(&table, &selective).await.unwrap();
    assert_eq!(numbers(&page.documents, "sk"), vec![json!(11)]);

    let invalid = DocumentQuery {
        filter: Some(json!({"KeyConditionExpression": "nope = :x"})),
        ..DocumentQuery::first_page(5)
    };
    assert_eq!(
        documents.find(&table, &invalid).await.unwrap_err().kind,
        ErrorKind::Query
    );
    delete_table(&table.name).await;
}

#[tokio::test]
async fn writes_round_trip_every_attribute_type() {
    let Some((connection, table)) = fixture("writes").await else {
        return;
    };
    let documents = documents(connection.as_ref());
    let item = json!({
        "pk": "item#1",
        "sk": 1,
        "precise": {"$N": "12345678901234567890.123456789"},
        "ratio": 0.25,
        "blob": {"$B": "AAEC/w=="},
        "tags": {"$SS": ["a", "b"]},
        "sizes": {"$NS": ["1", "2.5"]},
        "blobs": {"$BS": ["AQ=="]},
        "nested": {"list": [1, "two", null, true], "map": {"$M": {"$N": "literal"}}}
    });
    let inserted = documents
        .write(
            &table,
            &DocumentWrite::Insert {
                document: item.clone(),
            },
        )
        .await
        .unwrap();
    let key = inserted.key.unwrap();
    assert_eq!(key, json!({"pk": "item#1", "sk": 1}));
    let duplicate = documents
        .write(
            &table,
            &DocumentWrite::Insert {
                document: item.clone(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(duplicate.kind, ErrorKind::Query);

    let stored = documents
        .find(&table, &DocumentQuery::first_page(10))
        .await
        .unwrap();
    let body = &stored.documents[0].body;
    for field in ["precise", "ratio", "blob", "blobs", "nested", "pk", "sk"] {
        assert_eq!(body[field], item[field], "field {field}");
    }
    let mut tags = body["tags"]["$SS"].as_array().unwrap().clone();
    tags.sort_by_key(|tag| tag.to_string());
    assert_eq!(tags, vec![json!("a"), json!("b")]);

    let replaced = documents
        .write(
            &table,
            &DocumentWrite::Replace {
                key: key.clone(),
                document: json!({"label": "replaced"}),
            },
        )
        .await
        .unwrap();
    assert_eq!(replaced.affected, 1);
    let after = documents
        .find(&table, &DocumentQuery::first_page(10))
        .await
        .unwrap();
    assert_eq!(
        after.documents[0].body,
        json!({"pk": "item#1", "sk": 1, "label": "replaced"})
    );
    let key_change = documents
        .write(
            &table,
            &DocumentWrite::Replace {
                key: key.clone(),
                document: json!({"pk": "other", "sk": 1}),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(key_change.kind, ErrorKind::Query);

    documents
        .write(&table, &DocumentWrite::Delete { key: key.clone() })
        .await
        .unwrap();
    let missing = documents
        .write(&table, &DocumentWrite::Delete { key: key.clone() })
        .await
        .unwrap_err();
    assert_eq!(missing.kind, ErrorKind::ConcurrentModification);
    let gone = documents
        .write(
            &table,
            &DocumentWrite::Replace {
                key,
                document: json!({"label": "x"}),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(gone.kind, ErrorKind::ConcurrentModification);
    delete_table(&table.name).await;
}

#[tokio::test]
async fn global_secondary_indexes_are_created_and_dropped() {
    let Some((connection, table)) = fixture("indexes").await else {
        return;
    };
    let documents = documents(connection.as_ref());
    let created = documents
        .write(
            &table,
            &DocumentWrite::CreateIndex {
                definition: json!({
                    "IndexName": "byLabel",
                    "KeySchema": [{"AttributeName": "label", "KeyType": "HASH"},
                                  {"AttributeName": "sk", "KeyType": "RANGE"}],
                    "AttributeDefinitions": [{"AttributeName": "label", "AttributeType": "S"}],
                    "Projection": {"ProjectionType": "INCLUDE", "NonKeyAttributes": ["email"]}
                }),
            },
        )
        .await
        .unwrap();
    assert!(created.message.unwrap().contains("byLabel"));
    let index = wait_for_active_index(documents, &table, "byLabel").await;
    assert_eq!(
        index.definition["Projection"],
        json!({"ProjectionType": "INCLUDE", "NonKeyAttributes": ["email"]})
    );
    documents
        .write(
            &table,
            &DocumentWrite::DropIndex {
                name: "byLabel".into(),
            },
        )
        .await
        .unwrap();
    let details = documents.describe_collection(&table).await.unwrap();
    let remaining = details.indexes.iter().find(|index| index.name == "byLabel");
    if let Some(deleting) = remaining {
        let detail = deleting.detail.as_deref().unwrap_or_default();
        assert!(detail.contains("deleting"), "{detail}");
    }
    delete_table(&table.name).await;
}

#[tokio::test]
async fn partiql_statements_run_with_row_limits() {
    let Some((connection, table)) = fixture("partiql").await else {
        return;
    };
    seed(documents(connection.as_ref()), &table, 9).await;
    let executor = console(connection.as_ref());
    let name = &table.name;

    let limited = executor
        .execute(
            &format!("SELECT * FROM \"{name}\";"),
            &ExecuteOptions { row_limit: 4 },
        )
        .await
        .unwrap();
    let batch = rows(limited);
    assert_eq!(batch.rows.len(), 4);
    assert!(batch.has_more);
    let names: Vec<_> = batch.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["email", "label", "pk", "score", "sk"]);

    let all = rows(
        executor
            .execute(
                &format!("SELECT sk FROM \"{name}\" WHERE pk = 'user#0'"),
                &ExecuteOptions::default(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(
        all.rows,
        vec![
            vec![Value::Int(0)],
            vec![Value::Int(3)],
            vec![Value::Int(6)]
        ]
    );
    assert!(!all.has_more);

    let inserted = executor
        .execute(
            &format!("INSERT INTO \"{name}\" VALUE {{'pk': 'new', 'sk': 1, 'tags': <<'x'>>}}"),
            &ExecuteOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(inserted, StatementOutcome::Affected { rows: 1 });
    let tagged = rows(
        executor
            .execute(
                &format!("SELECT tags FROM \"{name}\" WHERE pk = 'new' AND sk = 1"),
                &ExecuteOptions::default(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(
        tagged.rows,
        vec![vec![Value::Text("{\"$SS\":[\"x\"]}".into())]]
    );

    let syntax = executor
        .execute("SELEC * FROM nowhere", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(syntax.kind, ErrorKind::Query);
    executor.cancel().await.unwrap();
    delete_table(&table.name).await;
}
