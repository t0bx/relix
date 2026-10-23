/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::extended_json::{document_from_json, from_json};
use crate::key_spec::ordered_keys;
use mongodb::bson::{Bson, Document};
use relix_driver_api::DriverError;
use serde_json::Value as Json;

/// Builds a `createIndexes` index specification. Accepted definitions are
/// `{"keys": {...}, "options": {...}}` (options such as `name`, `unique`,
/// `expireAfterSeconds`, `partialFilterExpression`) and the index document shape that
/// `listIndexes` returns (`{"key": {...}, "name": ..., "unique": true}`). Keys may be an
/// ordered array of pairs for compound indexes. Without a name, MongoDB's default
/// `field_direction` naming is used.
pub fn index_specification(definition: &Json) -> Result<Document, DriverError> {
    let Json::Object(fields) = definition else {
        return Err(DriverError::query(
            "an index definition must be an object like {\"keys\": {\"field\": 1}}",
        ));
    };
    let keys = fields
        .get("keys")
        .or_else(|| fields.get("key"))
        .ok_or_else(|| DriverError::query("the index definition needs \"keys\""))?;
    let keys = ordered_keys(Some(keys), "index keys")?;
    if keys.is_empty() {
        return Err(DriverError::query("an index needs at least one key"));
    }
    let mut specification = match fields.get("options") {
        Some(options) => document_from_json(options, "index options")?,
        None => inline_options(fields)?,
    };
    if !specification.contains_key("name") {
        specification.insert("name", default_index_name(&keys));
    }
    let mut ordered = Document::new();
    ordered.insert("key", keys);
    ordered.extend(specification);
    Ok(ordered)
}

fn inline_options(fields: &serde_json::Map<String, Json>) -> Result<Document, DriverError> {
    let mut options = Document::new();
    for (name, value) in fields {
        if !matches!(name.as_str(), "key" | "keys" | "v" | "ns") {
            options.insert(name.clone(), from_json(value)?);
        }
    }
    Ok(options)
}

pub fn default_index_name(keys: &Document) -> String {
    keys.iter()
        .map(|(field, direction)| {
            let direction = match direction {
                Bson::String(kind) => kind.clone(),
                Bson::Double(number) => format!("{number}"),
                other => other.clone().into_relaxed_extjson().to_string(),
            };
            format!("{field}_{direction}")
        })
        .collect::<Vec<_>>()
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn keys_and_options_definitions_are_accepted() {
        let specification = index_specification(&json!({
            "keys": [["email", 1], ["created", -1]],
            "options": {"unique": true}
        }))
        .unwrap();
        assert_eq!(
            specification,
            doc! {
                "key": { "email": 1, "created": -1 },
                "unique": true,
                "name": "email_1_created_-1",
            }
        );
    }

    #[test]
    fn list_indexes_shaped_definitions_are_accepted() {
        let specification = index_specification(&json!({
            "v": 2,
            "key": {"body": "text"},
            "name": "search",
            "expireAfterSeconds": 60
        }))
        .unwrap();
        assert_eq!(specification.get_str("name").unwrap(), "search");
        assert_eq!(
            specification.get_document("key").unwrap(),
            &doc! { "body": "text" }
        );
        assert!(!specification.contains_key("v"));
        assert_eq!(specification.get_i32("expireAfterSeconds").unwrap(), 60);
    }

    #[test]
    fn definitions_without_keys_are_rejected() {
        assert!(index_specification(&json!({"options": {}})).is_err());
        assert!(index_specification(&json!({"keys": {}})).is_err());
        assert!(index_specification(&json!([1])).is_err());
    }
}
