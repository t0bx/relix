/*
 * @author t0bx
 * @since 23.10.2026
 */

use mongodb::bson::{Bson, Document};
use relix_driver_api::DriverError;
use serde_json::{Map, Value as Json, json};

/// Converts a BSON value into relaxed Extended JSON for editing. 64-bit integers are kept
/// in their canonical `{"$numberLong": "…"}` form, because relaxed JSON turns small
/// int64 values into plain numbers that would come back as int32 and silently change the
/// stored type when a document is saved.
pub fn to_editable_json(value: Bson) -> Json {
    match value {
        Bson::Int64(number) => json!({ "$numberLong": number.to_string() }),
        Bson::Document(document) => document_to_editable_json(document),
        Bson::Array(items) => Json::Array(items.into_iter().map(to_editable_json).collect()),
        other => other.into_relaxed_extjson(),
    }
}

pub fn document_to_editable_json(document: Document) -> Json {
    Json::Object(
        document
            .into_iter()
            .map(|(name, value)| (name, to_editable_json(value)))
            .collect::<Map<_, _>>(),
    )
}

pub fn to_display_fields(document: Document) -> Vec<(String, Json)> {
    document
        .into_iter()
        .map(|(name, value)| (name, value.into_relaxed_extjson()))
        .collect()
}

pub fn from_json(value: &Json) -> Result<Bson, DriverError> {
    Bson::try_from(value.clone())
        .map_err(|error| DriverError::query(format!("invalid Extended JSON: {error}")))
}

pub fn document_from_json(value: &Json, purpose: &str) -> Result<Document, DriverError> {
    match from_json(value)? {
        Bson::Document(document) => Ok(document),
        _ => Err(DriverError::query(format!(
            "{purpose} must be a JSON object"
        ))),
    }
}

pub fn optional_document(value: Option<&Json>, purpose: &str) -> Result<Document, DriverError> {
    match value {
        None | Some(Json::Null) => Ok(Document::new()),
        Some(value) => document_from_json(value, purpose),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::oid::ObjectId;
    use mongodb::bson::{DateTime, doc};
    use pretty_assertions::assert_eq;

    #[test]
    fn documents_round_trip_without_changing_types() {
        let original = doc! {
            "_id": ObjectId::parse_str("65f0a1b2c3d4e5f601234567").unwrap(),
            "small_long": 5_i64,
            "int": 7_i32,
            "double": 2.0_f64,
            "created": DateTime::from_millis(1_700_000_000_000),
            "nested": { "list": [1_i64, "x", null] },
        };
        let json = document_to_editable_json(original.clone());
        assert_eq!(json["_id"], json!({"$oid": "65f0a1b2c3d4e5f601234567"}));
        assert_eq!(json["small_long"], json!({"$numberLong": "5"}));
        assert_eq!(json["created"], json!({"$date": "2023-11-14T22:13:20Z"}));
        let text = serde_json::to_string(&json).unwrap();
        let reparsed: Json = serde_json::from_str(&text).unwrap();
        let restored = document_from_json(&reparsed, "document").unwrap();
        for (name, value) in &original {
            assert_eq!(restored.get(name), Some(value), "field {name}");
        }
    }

    #[test]
    fn display_fields_use_relaxed_numbers_and_keep_order() {
        let fields = to_display_fields(doc! { "z": 5_i64, "a": "text" });
        assert_eq!(
            fields,
            vec![("z".to_owned(), json!(5)), ("a".to_owned(), json!("text"))]
        );
    }

    #[test]
    fn non_objects_are_rejected_where_documents_are_required() {
        let error = document_from_json(&json!([1]), "filter").unwrap_err();
        assert!(error.message.contains("filter"));
        assert!(
            optional_document(Some(&Json::Null), "sort")
                .unwrap()
                .is_empty()
        );
    }
}
