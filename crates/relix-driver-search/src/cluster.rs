/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::SearchClient;
use relix_driver_api::DriverError;
use relix_driver_http::Method;
use serde::Deserialize;
use serde_json::Value as Json;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RootInfo {
    #[serde(default)]
    pub name: Option<String>,
    pub cluster_name: String,
    #[serde(default)]
    pub cluster_uuid: Option<String>,
    pub version: VersionInfo,
    #[serde(default)]
    pub tagline: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct VersionInfo {
    pub number: String,
    #[serde(default)]
    pub distribution: Option<String>,
    #[serde(default)]
    pub build_flavor: Option<String>,
    #[serde(default)]
    pub lucene_version: Option<String>,
}

pub async fn root_info(client: &SearchClient) -> Result<RootInfo, DriverError> {
    let body = client.json(Method::GET, "/", None).await?;
    serde_json::from_value(body).map_err(|_| {
        DriverError::new(
            relix_driver_api::ErrorKind::Protocol,
            "the server did not identify itself as Elasticsearch or OpenSearch",
        )
    })
}

pub async fn cluster_health(client: &SearchClient) -> Result<Json, DriverError> {
    client.json(Method::GET, "/_cluster/health", None).await
}

pub fn cluster_properties(root: &RootInfo, health: Option<&Json>) -> Vec<(String, String)> {
    let mut properties = vec![("cluster".to_owned(), root.cluster_name.clone())];
    if let Some(uuid) = &root.cluster_uuid {
        properties.push(("cluster uuid".to_owned(), uuid.clone()));
    }
    if let Some(node) = &root.name {
        properties.push(("node".to_owned(), node.clone()));
    }
    if let Some(lucene) = &root.version.lucene_version {
        properties.push(("lucene version".to_owned(), lucene.clone()));
    }
    let Some(health) = health else {
        return properties;
    };
    for (field, label) in [
        ("status", "cluster health"),
        ("number_of_nodes", "nodes"),
        ("number_of_data_nodes", "data nodes"),
        ("active_shards", "active shards"),
        ("unassigned_shards", "unassigned shards"),
    ] {
        if let Some(value) = health.get(field) {
            properties.push((label.to_owned(), plain_text(value)));
        }
    }
    properties
}

pub fn plain_text(value: &Json) -> String {
    match value {
        Json::String(text) => text.clone(),
        Json::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cluster_properties_include_health() {
        let root: RootInfo = serde_json::from_value(json!({
            "name": "node-1", "cluster_name": "logs", "cluster_uuid": "abc",
            "version": {"number": "8.15.0", "lucene_version": "9.11.1"}
        }))
        .unwrap();
        let health = json!({"status": "yellow", "number_of_nodes": 1, "unassigned_shards": 2});
        let properties = cluster_properties(&root, Some(&health));
        assert!(properties.contains(&("cluster".into(), "logs".into())));
        assert!(properties.contains(&("cluster health".into(), "yellow".into())));
        assert!(properties.contains(&("unassigned shards".into(), "2".into())));
    }
}
