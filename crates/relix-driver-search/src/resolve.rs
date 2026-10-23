/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::SearchClient;
use crate::paths::segment;
use relix_core::CollectionKind;
use relix_driver_api::DriverError;
use relix_driver_http::Method;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Resolved {
    #[serde(default)]
    pub indices: Vec<ResolvedIndex>,
    #[serde(default)]
    pub aliases: Vec<ResolvedAlias>,
    #[serde(default)]
    pub data_streams: Vec<ResolvedDataStream>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ResolvedIndex {
    pub name: String,
    #[serde(default)]
    pub attributes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ResolvedAlias {
    pub name: String,
    #[serde(default)]
    pub indices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ResolvedDataStream {
    pub name: String,
    #[serde(default)]
    pub backing_indices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub kind: CollectionKind,
    pub members: Vec<String>,
}

impl ResolvedIndex {
    pub fn is_hidden(&self) -> bool {
        self.attributes
            .iter()
            .any(|attribute| attribute == "hidden" || attribute == "system")
    }
}

impl Resolved {
    pub fn target(&self, name: &str) -> Option<ResolvedTarget> {
        if let Some(stream) = self.data_streams.iter().find(|stream| stream.name == name) {
            return Some(ResolvedTarget {
                kind: CollectionKind::DataStream,
                members: stream.backing_indices.clone(),
            });
        }
        if let Some(alias) = self.aliases.iter().find(|alias| alias.name == name) {
            return Some(ResolvedTarget {
                kind: CollectionKind::Alias,
                members: alias.indices.clone(),
            });
        }
        self.indices
            .iter()
            .find(|index| index.name == name)
            .map(|index| ResolvedTarget {
                kind: CollectionKind::Index,
                members: vec![index.name.clone()],
            })
    }
}

pub async fn resolve(
    client: &SearchClient,
    pattern: &str,
    expand_wildcards: &str,
) -> Result<Resolved, DriverError> {
    let path = format!(
        "/_resolve/index/{}?expand_wildcards={expand_wildcards}",
        segment(pattern)?
    );
    let body = client.json(Method::GET, &path, None).await?;
    serde_json::from_value(body).map_err(|error| {
        DriverError::new(
            relix_driver_api::ErrorKind::Protocol,
            "unexpected resolve index response",
        )
        .with_detail(error.to_string())
    })
}

pub async fn resolve_target(
    client: &SearchClient,
    name: &str,
) -> Result<ResolvedTarget, DriverError> {
    resolve(client, name, "all")
        .await?
        .target(name)
        .ok_or_else(|| DriverError::query(format!("no index, alias or data stream named {name}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn targets_prefer_data_streams_then_aliases_then_indices() {
        let resolved: Resolved = serde_json::from_value(json!({
            "indices": [
                {"name": "orders-1", "aliases": ["orders"], "attributes": ["open"]},
                {"name": ".ds-logs-000001", "attributes": ["hidden", "open"], "data_stream": "logs"}
            ],
            "aliases": [{"name": "orders", "indices": ["orders-1"]}],
            "data_streams": [{"name": "logs", "backing_indices": [".ds-logs-000001"],
                "timestamp_field": "@timestamp"}]
        }))
        .unwrap();
        let stream = resolved.target("logs").unwrap();
        assert_eq!(stream.kind, CollectionKind::DataStream);
        assert_eq!(stream.members, vec![".ds-logs-000001"]);
        assert_eq!(
            resolved.target("orders").unwrap().kind,
            CollectionKind::Alias
        );
        assert_eq!(
            resolved.target("orders-1").unwrap().kind,
            CollectionKind::Index
        );
        assert!(resolved.target("missing").is_none());
        assert!(resolved.indices[1].is_hidden());
        assert!(!resolved.indices[0].is_hidden());
    }
}
