/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Product, cluster, collection, connect, documents, drop_quietly, run, write};
use relix_core::{CollectionKind, DocumentWrite};
use relix_driver_api::ErrorKind;
use relix_testing::integration::unique_name;
use serde_json::json;

fn has_property(properties: &[(String, String)], key: &str, value: &str) -> bool {
    properties
        .iter()
        .any(|(name, found)| name == key && found == value)
}

async fn catalog_lists_and_describes_indices_aliases_and_data_streams(product: Product) {
    let Some(connection) = connect(product).await else {
        return;
    };
    let connection = connection.as_ref();
    let base = unique_name("relix_catalog");
    let index = format!("{base}_orders");
    let alias = format!("{base}_alias");
    let stream = format!("{base}-logs");
    let template = format!("{base}_template");
    let orders = collection(connection, &index).await;
    let created = write(
        connection,
        &orders,
        DocumentWrite::CreateIndex {
            definition: json!({
                "name": index,
                "mappings": {"properties": {"status": {"type": "keyword"}, "total": {"type": "double"}}},
                "settings": {"number_of_shards": 1, "number_of_replicas": 0}
            }),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        created.message.as_deref(),
        Some(format!("index {index} created").as_str())
    );
    for (status, total) in [("paid", 10.5), ("open", 3.0), ("paid", 7.25)] {
        let document = json!({"status": status, "total": total});
        write(connection, &orders, DocumentWrite::Insert { document })
            .await
            .unwrap();
    }
    run(
        connection,
        &format!(
            "POST /_aliases\n{{\"actions\": [{{\"add\": {{\"index\": \"{index}\", \"alias\": \"{alias}\"}}}}]}}"
        ),
    )
    .await;
    run(
        connection,
        &format!(
            "PUT /_index_template/{template}\n{{\"index_patterns\": [\"{stream}*\"], \"data_stream\": {{}},\n\
             \"priority\": 500, \"template\": {{\"settings\": {{\"number_of_replicas\": 0}}}}}}"
        ),
    )
    .await;
    run(connection, &format!("PUT /_data_stream/{stream}")).await;
    let logs = collection(connection, &stream).await;
    let entry = json!({"@timestamp": "2026-10-23T10:00:00Z", "message": "hello"});
    let inserted = write(connection, &logs, DocumentWrite::Insert { document: entry })
        .await
        .unwrap();
    assert!(
        inserted.key.unwrap()["_index"]
            .as_str()
            .unwrap()
            .starts_with(".ds-")
    );

    let listed = documents(connection)
        .collections(&cluster(connection).await)
        .await
        .unwrap();
    let find = |name: &str| {
        listed
            .iter()
            .find(|info| info.collection.name == name)
            .unwrap_or_else(|| panic!("{name} missing"))
    };
    assert_eq!(find(&index).kind, CollectionKind::Index);
    assert_eq!(find(&index).document_count, Some(3));
    assert_eq!(find(&alias).kind, CollectionKind::Alias);
    assert_eq!(find(&alias).document_count, Some(3));
    assert_eq!(find(&stream).kind, CollectionKind::DataStream);
    assert_eq!(find(&stream).document_count, Some(1));
    assert!(
        listed
            .iter()
            .all(|info| !info.collection.name.starts_with('.'))
    );

    let details = documents(connection)
        .describe_collection(&orders)
        .await
        .unwrap();
    assert_eq!(details.kind, CollectionKind::Index);
    assert_eq!(details.key_fields, vec!["_index", "_id"]);
    assert_eq!(
        details.schema.as_ref().unwrap()["properties"]["status"]["type"],
        "keyword"
    );
    assert!(has_property(
        &details.properties,
        "index.number_of_shards",
        "1"
    ));
    assert!(has_property(&details.properties, "documents", "3"));
    assert!(has_property(
        &details.properties,
        "cluster",
        &cluster(connection).await
    ));
    assert!(
        details
            .properties
            .iter()
            .any(|(name, _)| name == "cluster health")
    );
    assert_eq!(details.indexes.len(), 1);

    let alias_ref = collection(connection, &alias).await;
    let alias_details = documents(connection)
        .describe_collection(&alias_ref)
        .await
        .unwrap();
    assert_eq!(alias_details.kind, CollectionKind::Alias);
    assert!(has_property(&alias_details.properties, "indices", &index));
    assert!(has_property(
        &alias_details.properties,
        "write index",
        &index
    ));

    let stream_details = documents(connection)
        .describe_collection(&logs)
        .await
        .unwrap();
    assert_eq!(stream_details.kind, CollectionKind::DataStream);
    assert!(has_property(
        &stream_details.properties,
        "timestamp field",
        "@timestamp"
    ));
    assert!(stream_details.indexes[0].name.starts_with(".ds-"));

    let refused = write(
        connection,
        &alias_ref,
        DocumentWrite::DropIndex {
            name: alias.clone(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(refused.kind, ErrorKind::Unsupported);
    let foreign = write(
        connection,
        &orders,
        DocumentWrite::DropIndex {
            name: "some-other-index".into(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(foreign.kind, ErrorKind::Query);
    let dropped_stream = write(
        connection,
        &logs,
        DocumentWrite::DropIndex {
            name: stream.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        dropped_stream.message.unwrap(),
        format!("data stream {stream} deleted")
    );
    write(
        connection,
        &orders,
        DocumentWrite::DropIndex {
            name: index.clone(),
        },
    )
    .await
    .unwrap();
    let remaining = documents(connection)
        .collections(&cluster(connection).await)
        .await
        .unwrap();
    assert!(
        remaining
            .iter()
            .all(|info| { ![&index, &alias, &stream].contains(&&info.collection.name) })
    );
    drop_quietly(connection, &format!("_index_template/{template}")).await;
}
for_each_product!(catalog_lists_and_describes_indices_aliases_and_data_streams);

async fn hidden_indices_are_listed_on_request(product: Product) {
    let Some(mut config) = common::config(product) else {
        return;
    };
    let connection = common::connect_with(product, &config).await;
    let hidden = unique_name("relix_hidden");
    run(
        connection.as_ref(),
        &format!(
            "PUT /{hidden}\n{{\"settings\": {{\"index.hidden\": true, \"number_of_replicas\": 0}}}}"
        ),
    )
    .await;
    let namespace = cluster(connection.as_ref()).await;
    let default_listing = documents(connection.as_ref())
        .collections(&namespace)
        .await
        .unwrap();
    assert!(
        default_listing
            .iter()
            .all(|info| info.collection.name != hidden)
    );
    config
        .settings
        .properties
        .insert("show_hidden".into(), "true".into());
    let revealing = common::connect_with(product, &config).await;
    let full_listing = documents(revealing.as_ref())
        .collections(&namespace)
        .await
        .unwrap();
    assert!(
        full_listing
            .iter()
            .any(|info| info.collection.name == hidden)
    );
    drop_quietly(connection.as_ref(), &hidden).await;
}
for_each_product!(hidden_indices_are_listed_on_request);
