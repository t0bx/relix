/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Product, collection, connect, documents, drop_quietly, key_id, run, write};
use relix_core::{CollectionRef, DocumentQuery, DocumentWrite};
use relix_driver_api::{DatabaseConnection, ErrorKind};
use relix_testing::integration::unique_name;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

async fn create_index(
    connection: &dyn DatabaseConnection,
    name: &str,
    extra: Json,
) -> CollectionRef {
    let mut definition = json!({
        "name": name,
        "mappings": {"properties": {"name": {"type": "keyword"}, "category": {"type": "keyword"},
                                    "n": {"type": "integer"}}},
        "settings": {"number_of_shards": 1, "number_of_replicas": 0}
    });
    if let (Some(target), Json::Object(extra)) = (definition["settings"].as_object_mut(), extra) {
        target.extend(extra);
    }
    let collection = collection(connection, name).await;
    write(
        connection,
        &collection,
        DocumentWrite::CreateIndex { definition },
    )
    .await
    .unwrap();
    collection
}

async fn bulk_numbers(connection: &dyn DatabaseConnection, index: &str, count: u32) {
    let mut request = format!("POST /{index}/_bulk?refresh=true\n");
    for n in 0..count {
        let category = if n % 2 == 0 { "even" } else { "odd" };
        request.push_str(&format!(
            "{{\"index\": {{\"_id\": \"doc-{n}\"}}}}\n{{\"n\": {n}, \"category\": \"{category}\"}}\n"
        ));
    }
    run(connection, &request).await;
}

async fn documents_round_trip_with_optimistic_concurrency(product: Product) {
    let Some(connection) = connect(product).await else {
        return;
    };
    let connection = connection.as_ref();
    let index = unique_name("relix_crud");
    let orders = create_index(connection, &index, json!({})).await;
    let first = write(
        connection,
        &orders,
        DocumentWrite::Insert {
            document: json!({"name": "a", "n": 1}),
        },
    )
    .await
    .unwrap();
    assert_eq!(first.affected, 1);
    let first_key = first.key.unwrap();
    assert_eq!(first_key["_index"], index.as_str());
    assert!(first_key["_seq_no"].is_u64() && first_key["_primary_term"].is_u64());
    let fixed = DocumentWrite::Insert {
        document: json!({"_id": "fixed", "name": "b", "n": 2}),
    };
    let fixed_outcome = write(connection, &orders, fixed.clone()).await.unwrap();
    assert_eq!(key_id(&fixed_outcome.key.unwrap()), "fixed");
    let duplicate = write(connection, &orders, fixed).await.unwrap_err();
    assert_eq!(duplicate.kind, ErrorKind::ConcurrentModification);

    let page = documents(connection)
        .find(
            &orders,
            &DocumentQuery {
                filter: Some(json!({"term": {"name": "a"}})),
                ..DocumentQuery::first_page(10)
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, Some(1));
    let read = page.documents[0].clone();
    assert_eq!(read.key, first_key);
    assert_eq!(read.body, json!({"name": "a", "n": 1}));

    let replaced = write(
        connection,
        &orders,
        DocumentWrite::Replace {
            key: read.key.clone(),
            document: json!({"name": "a", "n": 10}),
        },
    )
    .await
    .unwrap();
    assert_eq!(replaced.affected, 1);
    let new_key = replaced.key.unwrap();
    assert_ne!(new_key["_seq_no"], read.key["_seq_no"]);
    let stale = write(
        connection,
        &orders,
        DocumentWrite::Replace {
            key: read.key.clone(),
            document: json!({"name": "stale"}),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(stale.kind, ErrorKind::ConcurrentModification);
    let renamed = write(
        connection,
        &orders,
        DocumentWrite::Replace {
            key: new_key.clone(),
            document: json!({"_id": "other", "name": "a"}),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(renamed.kind, ErrorKind::Query);
    write(
        connection,
        &orders,
        DocumentWrite::Replace {
            key: json!("fixed"),
            document: json!({"name": "b", "n": 20}),
        },
    )
    .await
    .unwrap();

    let stale_delete = write(
        connection,
        &orders,
        DocumentWrite::Delete {
            key: read.key.clone(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(stale_delete.kind, ErrorKind::ConcurrentModification);
    let deleted = write(connection, &orders, DocumentWrite::Delete { key: new_key })
        .await
        .unwrap();
    assert_eq!(deleted.affected, 1);
    let missing = write(
        connection,
        &orders,
        DocumentWrite::Delete {
            key: json!(key_id(&first_key)),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(missing.kind, ErrorKind::ConcurrentModification);

    let remaining = documents(connection)
        .find(&orders, &DocumentQuery::first_page(10))
        .await
        .unwrap();
    assert_eq!(remaining.total, Some(1));
    assert_eq!(remaining.documents[0].body, json!({"name": "b", "n": 20}));
    drop_quietly(connection, &index).await;
}
for_each_product!(documents_round_trip_with_optimistic_concurrency);

async fn find_filters_sorts_and_projects(product: Product) {
    let Some(connection) = connect(product).await else {
        return;
    };
    let connection = connection.as_ref();
    let index = unique_name("relix_find");
    let numbers = create_index(connection, &index, json!({})).await;
    bulk_numbers(connection, &index, 10).await;
    let page = documents(connection)
        .find(
            &numbers,
            &DocumentQuery {
                filter: Some(json!({"query": {"term": {"category": "even"}}})),
                sort: Some(json!({"n": -1})),
                projection: Some(json!(["n"])),
                ..DocumentQuery::first_page(3)
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, Some(5));
    let values: Vec<Json> = page.documents.iter().map(|doc| doc.body.clone()).collect();
    assert_eq!(
        values,
        vec![json!({"n": 8}), json!({"n": 6}), json!({"n": 4})]
    );
    assert!(page.next_page.is_some());
    let rest = documents(connection)
        .find(
            &numbers,
            &DocumentQuery {
                filter: Some(json!({"term": {"category": "even"}})),
                sort: Some(json!({"n": -1})),
                projection: Some(json!({"category": 0})),
                page_token: page.next_page,
                limit: 3,
            },
        )
        .await
        .unwrap();
    let values: Vec<Json> = rest.documents.iter().map(|doc| doc.body.clone()).collect();
    assert_eq!(values, vec![json!({"n": 2}), json!({"n": 0})]);
    assert_eq!(rest.next_page, None);
    let invalid = documents(connection)
        .find(
            &numbers,
            &DocumentQuery {
                filter: Some(json!({"bogus_query": {}})),
                ..DocumentQuery::first_page(3)
            },
        )
        .await
        .unwrap_err();
    assert_eq!(invalid.kind, ErrorKind::Query);
    assert!(
        invalid.message.contains("bogus_query"),
        "{}",
        invalid.message
    );
    drop_quietly(connection, &index).await;
}
for_each_product!(find_filters_sorts_and_projects);

async fn read_all(
    connection: &dyn DatabaseConnection,
    collection: &CollectionRef,
    sort: Option<Json>,
    limit: u32,
) -> (Vec<String>, Vec<String>) {
    let mut ids = Vec::new();
    let mut tokens = Vec::new();
    let mut token = None;
    loop {
        let page = documents(connection)
            .find(
                collection,
                &DocumentQuery {
                    sort: sort.clone(),
                    page_token: token.clone(),
                    limit,
                    ..DocumentQuery::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(page.total, Some(55));
        ids.extend(page.documents.iter().map(|doc| key_id(&doc.key)));
        match page.next_page {
            Some(next) => {
                tokens.push(next.clone());
                token = Some(next);
            }
            None => return (ids, tokens),
        }
        assert!(ids.len() <= 55, "pagination does not terminate");
    }
}

async fn pagination_continues_past_the_result_window(product: Product) {
    let Some(connection) = connect(product).await else {
        return;
    };
    let connection = connection.as_ref();
    let index = unique_name("relix_pages");
    let numbers = create_index(connection, &index, json!({"max_result_window": 20})).await;
    bulk_numbers(connection, &index, 55).await;
    let (sorted, tokens) = read_all(connection, &numbers, Some(json!({"n": 1})), 8).await;
    let expected: Vec<String> = (0..55).map(|n| format!("doc-{n}")).collect();
    assert_eq!(sorted, expected);
    assert!(tokens.iter().any(|token| token.contains("\"after\"")));
    let (unsorted, _) = read_all(connection, &numbers, None, 7).await;
    assert_eq!(unsorted.len(), 55);
    assert_eq!(unsorted.iter().collect::<BTreeSet<_>>().len(), 55);
    let clamped = documents(connection)
        .find(&numbers, &DocumentQuery::first_page(0))
        .await
        .unwrap();
    assert_eq!(clamped.documents.len(), 20);
    drop_quietly(connection, &index).await;
}
for_each_product!(pagination_continues_past_the_result_window);
