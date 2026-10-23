/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::SearchClient;
use crate::cluster::{RootInfo, cluster_health, cluster_properties, plain_text};
use crate::document_key::KEY_FIELDS;
use crate::index_stats::{IndexStats, index_stats};
use crate::paths::segment;
use crate::resolve::{ResolvedTarget, resolve_target};
use relix_core::{CollectionDetails, CollectionKind, CollectionRef, DocumentIndex};
use relix_driver_api::DriverError;
use relix_driver_http::Method;
use serde_json::{Map, Value as Json, json};
use std::collections::HashMap;

const DATA_STREAM_FIELDS: [(&str, &str); 5] = [
    ("generation", "generation"),
    ("status", "data stream health"),
    ("template", "index template"),
    ("ilm_policy", "ILM policy"),
    ("hidden", "hidden"),
];

pub async fn describe_collection(
    client: &SearchClient,
    root: &RootInfo,
    collection: &CollectionRef,
) -> Result<CollectionDetails, DriverError> {
    let target = resolve_target(client, &collection.name).await?;
    let name = segment(&collection.name)?;
    let definitions = client
        .json(
            Method::GET,
            &format!("/{name}?flat_settings=true&expand_wildcards=all"),
            None,
        )
        .await?;
    let stats = index_stats(client, Some(&collection.name)).await?;
    let health = cluster_health(client).await.ok();
    let mut properties = vec![("kind".to_owned(), target.kind.label().to_owned())];
    properties.extend(cluster_properties(root, health.as_ref()));
    match target.kind {
        CollectionKind::DataStream => {
            properties.extend(data_stream_properties(client, &name).await?);
        }
        CollectionKind::Alias => {
            properties.push(("indices".to_owned(), target.members.join(", ")));
            if let Some(write_index) = alias_write_index(&definitions, &collection.name) {
                properties.push(("write index".to_owned(), write_index));
            }
        }
        _ => {}
    }
    if let [only] = target.members.as_slice() {
        properties.extend(
            stats
                .get(only)
                .map(IndexStats::properties)
                .unwrap_or_default(),
        );
        properties.extend(flat_settings(&definitions[only]["settings"]));
    }
    Ok(CollectionDetails {
        collection: collection.clone(),
        kind: target.kind,
        key_fields: KEY_FIELDS.iter().map(|field| (*field).to_owned()).collect(),
        indexes: member_indexes(&target, &definitions, &stats),
        schema: Some(mapping_schema(&target, &definitions)),
        properties,
    })
}

fn member_indexes(
    target: &ResolvedTarget,
    definitions: &Json,
    stats: &HashMap<String, IndexStats>,
) -> Vec<DocumentIndex> {
    target
        .members
        .iter()
        .map(|member| {
            let definition = &definitions[member];
            let detail = stats.get(member).map(|stats| {
                let documents = stats.docs_count.as_deref().unwrap_or("?");
                format!("{} · {documents} docs", stats.summary())
            });
            DocumentIndex {
                name: member.clone(),
                definition: json!({
                    "aliases": definition.get("aliases").cloned().unwrap_or(Json::Null),
                    "settings": definition.get("settings").cloned().unwrap_or(Json::Null),
                }),
                unique: false,
                detail,
            }
        })
        .collect()
}

fn mapping_schema(target: &ResolvedTarget, definitions: &Json) -> Json {
    let mappings = |member: &String| {
        definitions[member]
            .get("mappings")
            .cloned()
            .unwrap_or_else(|| json!({}))
    };
    match target.members.as_slice() {
        [only] => mappings(only),
        members => Json::Object(
            members
                .iter()
                .map(|member| (member.clone(), mappings(member)))
                .collect::<Map<_, _>>(),
        ),
    }
}

fn flat_settings(settings: &Json) -> Vec<(String, String)> {
    settings
        .as_object()
        .map(|settings| {
            settings
                .iter()
                .map(|(key, value)| (key.clone(), plain_text(value)))
                .collect()
        })
        .unwrap_or_default()
}

fn alias_write_index(definitions: &Json, alias: &str) -> Option<String> {
    let indices = definitions.as_object()?;
    let mut members = indices
        .iter()
        .filter(|(_, definition)| definition["aliases"].get(alias).is_some());
    let explicit = indices.iter().find(|(_, definition)| {
        definition["aliases"][alias]["is_write_index"].as_bool() == Some(true)
    });
    match explicit {
        Some((index, _)) => Some(index.clone()),
        None => match (members.next(), members.next()) {
            (Some((index, _)), None) => Some(index.clone()),
            _ => None,
        },
    }
}

async fn data_stream_properties(
    client: &SearchClient,
    name: &str,
) -> Result<Vec<(String, String)>, DriverError> {
    let body = client
        .json(Method::GET, &format!("/_data_stream/{name}"), None)
        .await?;
    let stream = &body["data_streams"][0];
    let mut properties: Vec<(String, String)> = DATA_STREAM_FIELDS
        .iter()
        .filter_map(|(field, label)| {
            stream
                .get(*field)
                .map(|value| ((*label).to_owned(), plain_text(value)))
        })
        .collect();
    if let Some(field) = stream["timestamp_field"]["name"].as_str() {
        properties.push(("timestamp field".to_owned(), field.to_owned()));
    }
    Ok(properties)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definitions() -> Json {
        json!({
            "orders-1": {"aliases": {"orders": {}}, "mappings": {"properties": {"a": {"type": "keyword"}}},
                         "settings": {"index.number_of_shards": "1"}},
            "orders-2": {"aliases": {"orders": {"is_write_index": true}}, "mappings": {},
                         "settings": {}}
        })
    }

    #[test]
    fn single_member_schema_is_its_mapping() {
        let target = ResolvedTarget {
            kind: CollectionKind::Index,
            members: vec!["orders-1".into()],
        };
        let schema = mapping_schema(&target, &definitions());
        assert_eq!(schema["properties"]["a"]["type"], "keyword");
        let settings = flat_settings(&definitions()["orders-1"]["settings"]);
        assert_eq!(
            settings,
            vec![("index.number_of_shards".into(), "1".into())]
        );
    }

    #[test]
    fn multi_member_schema_is_keyed_by_index() {
        let target = ResolvedTarget {
            kind: CollectionKind::Alias,
            members: vec!["orders-1".into(), "orders-2".into()],
        };
        let schema = mapping_schema(&target, &definitions());
        assert!(schema["orders-1"]["properties"].is_object());
        let indexes = member_indexes(&target, &definitions(), &HashMap::new());
        assert_eq!(indexes.len(), 2);
        assert_eq!(
            indexes[1].definition["aliases"]["orders"]["is_write_index"],
            true
        );
    }

    #[test]
    fn write_index_is_explicit_or_the_only_member() {
        assert_eq!(
            alias_write_index(&definitions(), "orders").as_deref(),
            Some("orders-2")
        );
        let single = json!({"a": {"aliases": {"x": {}}}});
        assert_eq!(alias_write_index(&single, "x").as_deref(), Some("a"));
        let ambiguous = json!({"a": {"aliases": {"x": {}}}, "b": {"aliases": {"x": {}}}});
        assert_eq!(alias_write_index(&ambiguous, "x"), None);
    }
}
