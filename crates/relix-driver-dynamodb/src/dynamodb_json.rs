/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::attribute_json::json_to_attribute;
use aws_sdk_dynamodb::primitives::Blob;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_smithy_types::base64;
use relix_driver_api::DriverError;
use serde_json::{Map, Value as Json};

const DESCRIPTORS: [&str; 10] = ["S", "N", "B", "BOOL", "NULL", "M", "L", "SS", "NS", "BS"];

/// Reads an expression attribute value. A single-key object whose key is a DynamoDB type
/// descriptor (`{"S": "x"}`, `{"N": "1"}`, `{"M": {...}}`, ...) is DynamoDB JSON; any
/// other value uses the plain JSON mapping of [`json_to_attribute`]. A plain map that
/// happens to have a single descriptor key must therefore be written as DynamoDB JSON
/// (`{"M": {"S": {"S": "x"}}}`).
pub fn expression_value(value: &Json) -> Result<AttributeValue, DriverError> {
    match value {
        Json::Object(object) if is_descriptor(object) => typed_value(object),
        other => json_to_attribute(other),
    }
}

fn is_descriptor(object: &Map<String, Json>) -> bool {
    object.len() == 1 && object.keys().all(|key| DESCRIPTORS.contains(&key.as_str()))
}

fn typed_value(object: &Map<String, Json>) -> Result<AttributeValue, DriverError> {
    let Some((descriptor, value)) = object.iter().next() else {
        return Err(DriverError::internal("DynamoDB JSON value without a type"));
    };
    let invalid = || {
        DriverError::query(format!(
            "invalid DynamoDB JSON value for type {descriptor}: {value}"
        ))
    };
    Ok(match (descriptor.as_str(), value) {
        ("S", Json::String(text)) => AttributeValue::S(text.clone()),
        ("N", Json::String(number)) => AttributeValue::N(number.clone()),
        ("N", Json::Number(number)) => AttributeValue::N(number.to_string()),
        ("B", Json::String(encoded)) => AttributeValue::B(blob(encoded).ok_or_else(invalid)?),
        ("BOOL", Json::Bool(flag)) => AttributeValue::Bool(*flag),
        ("NULL", Json::Bool(true)) => AttributeValue::Null(true),
        ("M", Json::Object(map)) => AttributeValue::M(
            map.iter()
                .map(|(name, value)| match value {
                    Json::Object(inner) if is_descriptor(inner) => {
                        Ok((name.clone(), typed_value(inner)?))
                    }
                    _ => Err(invalid()),
                })
                .collect::<Result<_, _>>()?,
        ),
        ("L", Json::Array(items)) => AttributeValue::L(
            items
                .iter()
                .map(|item| match item {
                    Json::Object(inner) if is_descriptor(inner) => typed_value(inner),
                    _ => Err(invalid()),
                })
                .collect::<Result<_, _>>()?,
        ),
        ("SS", Json::Array(items)) => AttributeValue::Ss(texts(items).ok_or_else(invalid)?),
        ("NS", Json::Array(items)) => AttributeValue::Ns(texts(items).ok_or_else(invalid)?),
        ("BS", Json::Array(items)) => AttributeValue::Bs(
            texts(items)
                .ok_or_else(invalid)?
                .iter()
                .map(|encoded| blob(encoded))
                .collect::<Option<_>>()
                .ok_or_else(invalid)?,
        ),
        _ => return Err(invalid()),
    })
}

fn texts(items: &[Json]) -> Option<Vec<String>> {
    items
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

fn blob(encoded: &str) -> Option<Blob> {
    base64::decode(encoded).ok().map(Blob::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn dynamodb_json_and_plain_json_are_both_accepted() {
        assert_eq!(
            expression_value(&json!({"N": "10"})).unwrap(),
            AttributeValue::N("10".into())
        );
        assert_eq!(
            expression_value(&json!(10)).unwrap(),
            AttributeValue::N("10".into())
        );
        assert_eq!(
            expression_value(&json!({"M": {"city": {"S": "Berlin"}}})).unwrap(),
            AttributeValue::M(HashMap::from([(
                "city".to_owned(),
                AttributeValue::S("Berlin".into())
            )]))
        );
        assert_eq!(
            expression_value(&json!({"city": "Berlin", "zip": 10115})).unwrap(),
            AttributeValue::M(HashMap::from([
                ("city".to_owned(), AttributeValue::S("Berlin".into())),
                ("zip".to_owned(), AttributeValue::N("10115".into())),
            ]))
        );
        assert_eq!(
            expression_value(&json!({"SS": ["a", "b"]})).unwrap(),
            AttributeValue::Ss(vec!["a".into(), "b".into()])
        );
    }

    #[test]
    fn malformed_dynamodb_json_is_rejected() {
        assert!(expression_value(&json!({"S": 1})).is_err());
        assert!(expression_value(&json!({"L": ["plain"]})).is_err());
        assert!(expression_value(&json!({"NULL": false})).is_err());
    }
}
