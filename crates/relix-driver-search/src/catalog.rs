/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::SearchClient;
use crate::index_stats::{IndexStats, index_stats, summed_count};
use crate::options::SearchOptions;
use crate::resolve::{Resolved, resolve};
use relix_core::{CollectionInfo, CollectionKind, CollectionRef};
use relix_driver_api::DriverError;
use std::collections::HashMap;

pub async fn collections(
    client: &SearchClient,
    namespace: &str,
    options: SearchOptions,
) -> Result<Vec<CollectionInfo>, DriverError> {
    let resolved = resolve(client, "*", options.expand_wildcards()).await?;
    let stats = index_stats(client, None).await?;
    Ok(collection_infos(namespace, &resolved, &stats, options))
}

pub fn collection_infos(
    namespace: &str,
    resolved: &Resolved,
    stats: &HashMap<String, IndexStats>,
    options: SearchOptions,
) -> Vec<CollectionInfo> {
    let visible = |name: &str| options.show_hidden || !name.starts_with('.');
    let info = |name: &str, kind, document_count, detail: String| CollectionInfo {
        collection: CollectionRef::new(namespace, name),
        kind,
        document_count,
        detail: Some(detail).filter(|detail| !detail.is_empty()),
    };
    let indices = resolved
        .indices
        .iter()
        .filter(|index| visible(&index.name) && (options.show_hidden || !index.is_hidden()))
        .map(|index| {
            let stats = stats.get(&index.name);
            info(
                &index.name,
                CollectionKind::Index,
                stats.and_then(IndexStats::document_count),
                stats.map(IndexStats::summary).unwrap_or_default(),
            )
        });
    let aliases = resolved
        .aliases
        .iter()
        .filter(|alias| visible(&alias.name))
        .map(|alias| {
            info(
                &alias.name,
                CollectionKind::Alias,
                summed_count(stats, &alias.indices),
                format!("→ {}", alias.indices.join(", ")),
            )
        });
    let streams = resolved
        .data_streams
        .iter()
        .filter(|stream| visible(&stream.name))
        .map(|stream| {
            info(
                &stream.name,
                CollectionKind::DataStream,
                summed_count(stats, &stream.backing_indices),
                format!("{} backing indices", stream.backing_indices.len()),
            )
        });
    let mut collections: Vec<CollectionInfo> = indices.chain(aliases).chain(streams).collect();
    collections.sort_by(|left, right| left.collection.name.cmp(&right.collection.name));
    collections
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn resolved() -> Resolved {
        serde_json::from_value(json!({
            "indices": [
                {"name": "orders-1", "attributes": ["open"]},
                {"name": ".internal", "attributes": ["open"]},
                {"name": ".ds-logs-000001", "attributes": ["hidden", "open"]}
            ],
            "aliases": [{"name": "orders", "indices": ["orders-1"]}],
            "data_streams": [{"name": "logs", "backing_indices": [".ds-logs-000001"]}]
        }))
        .unwrap()
    }

    fn stats() -> HashMap<String, IndexStats> {
        let rows: Vec<IndexStats> = serde_json::from_value(json!([
            {"index": "orders-1", "health": "green", "status": "open", "docs.count": "5"},
            {"index": ".ds-logs-000001", "docs.count": "9"}
        ]))
        .unwrap();
        rows.into_iter()
            .map(|row| (row.index.clone(), row))
            .collect()
    }

    #[test]
    fn lists_indices_aliases_and_data_streams_with_counts() {
        let infos = collection_infos("cluster", &resolved(), &stats(), SearchOptions::default());
        let summary: Vec<_> = infos
            .iter()
            .map(|info| {
                (
                    info.collection.name.as_str(),
                    info.kind,
                    info.document_count,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("logs", CollectionKind::DataStream, Some(9)),
                ("orders", CollectionKind::Alias, Some(5)),
                ("orders-1", CollectionKind::Index, Some(5)),
            ]
        );
        assert_eq!(infos[2].collection.namespace, "cluster");
        assert_eq!(infos[2].detail.as_deref(), Some("green · open"));
    }

    #[test]
    fn hidden_indices_are_shown_on_request() {
        let options = SearchOptions {
            show_hidden: true,
            ..SearchOptions::default()
        };
        let infos = collection_infos("cluster", &resolved(), &stats(), options);
        assert_eq!(infos.len(), 5);
    }
}
