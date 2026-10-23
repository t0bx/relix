/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::extended_json::{from_json, optional_document};
use mongodb::bson::Document;
use relix_driver_api::DriverError;
use serde_json::Value as Json;

/// Parses a sort or index key specification. JSON objects reach the driver with their
/// fields sorted alphabetically, so compound specifications whose order matters can be
/// given as an ordered array of pairs (`[["age", -1], ["name", 1]]`) or of single-field
/// objects (`[{"age": -1}, {"name": 1}]`); a plain object works for one field.
pub fn ordered_keys(spec: Option<&Json>, purpose: &str) -> Result<Document, DriverError> {
    let Some(Json::Array(entries)) = spec else {
        return optional_document(spec, purpose);
    };
    let mut document = Document::new();
    for entry in entries {
        match entry {
            Json::Array(pair) if pair.len() == 2 => {
                let Json::String(field) = &pair[0] else {
                    return Err(invalid_entry(purpose));
                };
                document.insert(field.clone(), from_json(&pair[1])?);
            }
            Json::Object(fields) => {
                for (field, direction) in fields {
                    document.insert(field.clone(), from_json(direction)?);
                }
            }
            _ => return Err(invalid_entry(purpose)),
        }
    }
    Ok(document)
}

fn invalid_entry(purpose: &str) -> DriverError {
    DriverError::query(format!(
        "{purpose} entries must be [\"field\", value] pairs or {{\"field\": value}} objects"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn compound_specifications_keep_their_order() {
        let sort = ordered_keys(Some(&json!([["z", -1], {"a": 1}])), "sort").unwrap();
        assert_eq!(sort, doc! { "z": -1, "a": 1 });
        assert_eq!(
            ordered_keys(Some(&json!({"a": -1})), "sort").unwrap(),
            doc! { "a": -1 }
        );
        assert!(ordered_keys(Some(&json!([1])), "sort").is_err());
        assert!(ordered_keys(None, "sort").unwrap().is_empty());
    }
}
