/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::SearchClient;
use crate::paths::segment;
use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::Method;
use serde::Deserialize;
use std::collections::HashMap;

const COLUMNS: &str = "index,uuid,health,status,pri,rep,docs.count,docs.deleted,store.size";

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct IndexStats {
    pub index: String,
    pub uuid: Option<String>,
    pub health: Option<String>,
    pub status: Option<String>,
    pub pri: Option<String>,
    pub rep: Option<String>,
    #[serde(rename = "docs.count")]
    pub docs_count: Option<String>,
    #[serde(rename = "docs.deleted")]
    pub docs_deleted: Option<String>,
    #[serde(rename = "store.size")]
    pub store_size: Option<String>,
}

impl IndexStats {
    pub fn document_count(&self) -> Option<u64> {
        self.docs_count.as_deref()?.parse().ok()
    }

    pub fn summary(&self) -> String {
        [
            self.health.as_deref(),
            self.status.as_deref(),
            self.store_size.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
    }

    pub fn properties(&self) -> Vec<(String, String)> {
        [
            ("health", &self.health),
            ("status", &self.status),
            ("primary shards", &self.pri),
            ("replicas", &self.rep),
            ("documents", &self.docs_count),
            ("deleted documents", &self.docs_deleted),
            ("store size", &self.store_size),
            ("uuid", &self.uuid),
        ]
        .into_iter()
        .filter_map(|(label, value)| Some((label.to_owned(), value.clone()?)))
        .collect()
    }
}

/// Reads `_cat/indices` for every index (hidden ones included, so data stream and alias
/// counts can be summed from their members). Document counts are Lucene counts, so
/// nested documents are included. A user without the `monitor` privilege still gets a
/// listing, just without statistics.
pub async fn index_stats(
    client: &SearchClient,
    pattern: Option<&str>,
) -> Result<HashMap<String, IndexStats>, DriverError> {
    let target = match pattern {
        Some(pattern) => format!("/{}", segment(pattern)?),
        None => String::new(),
    };
    let path = format!("/_cat/indices{target}?format=json&h={COLUMNS}&expand_wildcards=all");
    let rows = match client.json(Method::GET, &path, None).await {
        Ok(rows) => rows,
        Err(error) if error.kind == ErrorKind::Authentication => return Ok(HashMap::new()),
        Err(error) => return Err(error),
    };
    let rows: Vec<IndexStats> = serde_json::from_value(rows).map_err(|error| {
        DriverError::new(ErrorKind::Protocol, "unexpected _cat/indices response")
            .with_detail(error.to_string())
    })?;
    Ok(rows
        .into_iter()
        .map(|row| (row.index.clone(), row))
        .collect())
}

pub fn summed_count<'a>(
    stats: &HashMap<String, IndexStats>,
    members: impl IntoIterator<Item = &'a String>,
) -> Option<u64> {
    members
        .into_iter()
        .map(|member| stats.get(member).and_then(IndexStats::document_count))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stats() -> HashMap<String, IndexStats> {
        let rows: Vec<IndexStats> = serde_json::from_value(json!([
            {"index": "a", "health": "green", "status": "open", "docs.count": "3",
             "store.size": "5kb", "pri": "1", "rep": "0"},
            {"index": "b", "health": "yellow", "status": "open", "docs.count": "4"},
            {"index": "closed", "health": null, "status": "close", "docs.count": null}
        ]))
        .unwrap();
        rows.into_iter()
            .map(|row| (row.index.clone(), row))
            .collect()
    }

    #[test]
    fn member_counts_are_summed() {
        let stats = stats();
        let members = ["a".to_owned(), "b".to_owned()];
        assert_eq!(summed_count(&stats, &members), Some(7));
        let with_closed = ["a".to_owned(), "closed".to_owned()];
        assert_eq!(summed_count(&stats, &with_closed), None);
    }

    #[test]
    fn summaries_and_properties_skip_missing_values() {
        let stats = stats();
        assert_eq!(stats["a"].summary(), "green · open · 5kb");
        assert_eq!(stats["closed"].summary(), "close");
        let properties = stats["a"].properties();
        assert!(properties.contains(&("documents".into(), "3".into())));
        assert!(!properties.iter().any(|(label, _)| label == "uuid"));
    }
}
