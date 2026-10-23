/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::DriverError;
use serde_json::{Map, Value as Json};

pub const KEY_FIELDS: [&str; 2] = ["_index", "_id"];

/// Identifies a document: the concrete index it lives in, its `_id`, and the sequence
/// number and primary term it had when it was read, which make replaces and deletes
/// conditional (`if_seq_no` / `if_primary_term`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentKey {
    pub index: Option<String>,
    pub id: String,
    pub seq_no: Option<u64>,
    pub primary_term: Option<u64>,
}

impl DocumentKey {
    /// Accepts the key objects produced by `find` and write outcomes, or a bare `_id`
    /// string, which then addresses the document through the collection name.
    pub fn parse(key: &Json) -> Result<Self, DriverError> {
        match key {
            Json::String(id) if !id.is_empty() => Ok(Self {
                index: None,
                id: id.clone(),
                seq_no: None,
                primary_term: None,
            }),
            Json::Object(_) => Self::from_metadata(key).ok_or_else(invalid_key),
            _ => Err(invalid_key()),
        }
    }

    pub fn from_metadata(source: &Json) -> Option<Self> {
        let id = match source.get("_id")? {
            Json::String(id) if !id.is_empty() => id.clone(),
            Json::Number(number) => number.to_string(),
            _ => return None,
        };
        let index = source
            .get("_index")
            .and_then(Json::as_str)
            .map(str::to_owned);
        let seq_no = source.get("_seq_no").and_then(Json::as_u64);
        let primary_term = source.get("_primary_term").and_then(Json::as_u64);
        Some(Self {
            index,
            id,
            seq_no,
            primary_term,
        })
    }

    pub fn index_or<'a>(&'a self, collection: &'a str) -> &'a str {
        self.index.as_deref().unwrap_or(collection)
    }

    pub fn concurrency_query(&self) -> String {
        match (self.seq_no, self.primary_term) {
            (Some(seq_no), Some(primary_term)) => {
                format!("&if_seq_no={seq_no}&if_primary_term={primary_term}")
            }
            _ => String::new(),
        }
    }

    pub fn to_json(&self) -> Json {
        let mut key = Map::new();
        if let Some(index) = &self.index {
            key.insert("_index".to_owned(), Json::from(index.clone()));
        }
        key.insert("_id".to_owned(), Json::from(self.id.clone()));
        if let (Some(seq_no), Some(primary_term)) = (self.seq_no, self.primary_term) {
            key.insert("_seq_no".to_owned(), Json::from(seq_no));
            key.insert("_primary_term".to_owned(), Json::from(primary_term));
        }
        Json::Object(key)
    }
}

fn invalid_key() -> DriverError {
    DriverError::query(
        "document key must be an _id string or {\"_index\": ..., \"_id\": ...} \
         (optionally with _seq_no and _primary_term)",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hit_metadata_becomes_a_conditional_key() {
        let hit = json!({"_index": "orders", "_id": "7", "_seq_no": 4, "_primary_term": 1,
            "_source": {"a": 1}});
        let key = DocumentKey::from_metadata(&hit).unwrap();
        assert_eq!(key.index_or("alias"), "orders");
        assert_eq!(key.concurrency_query(), "&if_seq_no=4&if_primary_term=1");
        assert_eq!(
            key.to_json(),
            json!({"_index": "orders", "_id": "7", "_seq_no": 4, "_primary_term": 1})
        );
        assert_eq!(DocumentKey::parse(&key.to_json()).unwrap(), key);
    }

    #[test]
    fn bare_ids_use_the_collection_without_conditions() {
        let key = DocumentKey::parse(&json!("abc")).unwrap();
        assert_eq!(key.index_or("orders"), "orders");
        assert_eq!(key.concurrency_query(), "");
        assert_eq!(key.to_json(), json!({"_id": "abc"}));
    }

    #[test]
    fn malformed_keys_are_rejected() {
        for key in [json!(""), json!(5), json!({"_index": "a"}), json!(null)] {
            assert!(DocumentKey::parse(&key).is_err(), "{key}");
        }
    }
}
