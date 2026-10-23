/*
 * @author t0bx
 * @since 23.10.2026
 */

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionKind {
    Collection,
    View,
    TimeSeries,
    Index,
    Alias,
    DataStream,
    Table,
}

impl CollectionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Collection => "collection",
            Self::View => "view",
            Self::TimeSeries => "time series",
            Self::Index => "index",
            Self::Alias => "alias",
            Self::DataStream => "data stream",
            Self::Table => "table",
        }
    }

    pub fn is_writable(self) -> bool {
        !matches!(self, Self::View)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CollectionRef {
    pub namespace: String,
    pub name: String,
}

impl CollectionRef {
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
        }
    }
}

impl fmt::Display for CollectionRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.namespace.is_empty() {
            f.write_str(&self.name)
        } else {
            write!(f, "{}.{}", self.namespace, self.name)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionInfo {
    pub collection: CollectionRef,
    pub kind: CollectionKind,
    pub document_count: Option<u64>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentIndex {
    pub name: String,
    pub definition: Json,
    pub unique: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionDetails {
    pub collection: CollectionRef,
    pub kind: CollectionKind,
    pub key_fields: Vec<String>,
    pub indexes: Vec<DocumentIndex>,
    pub schema: Option<Json>,
    pub properties: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DocumentQuery {
    pub filter: Option<Json>,
    pub sort: Option<Json>,
    pub projection: Option<Json>,
    pub page_token: Option<String>,
    pub limit: u32,
}

impl DocumentQuery {
    pub fn first_page(limit: u32) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub key: Json,
    pub body: Json,
}

impl Document {
    pub fn key_label(&self) -> String {
        match &self.key {
            Json::String(text) => text.clone(),
            other => other.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DocumentPage {
    pub documents: Vec<Document>,
    pub next_page: Option<String>,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DocumentWrite {
    Insert { document: Json },
    Replace { key: Json, document: Json },
    Delete { key: Json },
    CreateIndex { definition: Json },
    DropIndex { name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentWriteOutcome {
    pub affected: u64,
    pub key: Option<Json>,
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_labels_unwrap_strings() {
        let by_string = Document {
            key: json!("abc"),
            body: json!({}),
        };
        let by_object = Document {
            key: json!({"$oid": "65f0"}),
            body: json!({}),
        };
        assert_eq!(by_string.key_label(), "abc");
        assert_eq!(by_object.key_label(), "{\"$oid\":\"65f0\"}");
    }

    #[test]
    fn collection_refs_display_with_namespace() {
        assert_eq!(
            CollectionRef::new("shop", "orders").to_string(),
            "shop.orders"
        );
        assert_eq!(CollectionRef::new("", "logs-2026").to_string(), "logs-2026");
        assert!(!CollectionKind::View.is_writable());
    }
}
