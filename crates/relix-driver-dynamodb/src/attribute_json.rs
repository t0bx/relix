/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::number_text::exact_json_number;
use aws_sdk_dynamodb::primitives::Blob;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_smithy_types::base64;
use relix_driver_api::DriverError;
use serde_json::{Map, Value as Json, json};
use std::collections::HashMap;

pub type Item = HashMap<String, AttributeValue>;

const NUMBER_TAG: &str = "$N";
const BINARY_TAG: &str = "$B";
const STRING_SET_TAG: &str = "$SS";
const NUMBER_SET_TAG: &str = "$NS";
const BINARY_SET_TAG: &str = "$BS";
const MAP_TAG: &str = "$M";
const TAGS: [&str; 6] = [
    NUMBER_TAG,
    BINARY_TAG,
    STRING_SET_TAG,
    NUMBER_SET_TAG,
    BINARY_SET_TAG,
    MAP_TAG,
];

/// Converts an attribute value into plain JSON. Strings, booleans, null, lists and maps
/// map directly; numbers become JSON numbers when exact and `{"$N": "…"}` otherwise.
/// Types JSON lacks are tagged: binary `{"$B": base64}`, string sets `{"$SS": [...]}`,
/// number sets `{"$NS": ["1", ...]}` (text keeps full precision) and binary sets
/// `{"$BS": [base64, ...]}`. A map whose only key is one of these tags is wrapped as
/// `{"$M": {...}}` so it cannot be mistaken for a tagged value.
pub fn attribute_to_json(value: &AttributeValue) -> Json {
    match value {
        AttributeValue::S(text) => Json::String(text.clone()),
        AttributeValue::N(number) => {
            exact_json_number(number).unwrap_or_else(|| json!({ NUMBER_TAG: number }))
        }
        AttributeValue::Bool(flag) => Json::Bool(*flag),
        AttributeValue::Null(_) => Json::Null,
        AttributeValue::B(blob) => json!({ BINARY_TAG: base64::encode(blob.as_ref()) }),
        AttributeValue::Ss(items) => json!({ STRING_SET_TAG: items }),
        AttributeValue::Ns(items) => json!({ NUMBER_SET_TAG: items }),
        AttributeValue::Bs(items) => {
            let encoded: Vec<String> = items
                .iter()
                .map(|blob| base64::encode(blob.as_ref()))
                .collect();
            json!({ BINARY_SET_TAG: encoded })
        }
        AttributeValue::L(items) => Json::Array(items.iter().map(attribute_to_json).collect()),
        AttributeValue::M(map) => {
            let object = item_to_json_map(map);
            if is_tagged(&object) {
                json!({ MAP_TAG: object })
            } else {
                Json::Object(object)
            }
        }
        _ => Json::Null,
    }
}

pub fn item_to_json(item: &Item) -> Json {
    Json::Object(item_to_json_map(item))
}

fn item_to_json_map(item: &Item) -> Map<String, Json> {
    item.iter()
        .map(|(name, value)| (name.clone(), attribute_to_json(value)))
        .collect()
}

pub fn json_to_attribute(value: &Json) -> Result<AttributeValue, DriverError> {
    Ok(match value {
        Json::Null => AttributeValue::Null(true),
        Json::Bool(flag) => AttributeValue::Bool(*flag),
        Json::Number(number) => AttributeValue::N(number.to_string()),
        Json::String(text) => AttributeValue::S(text.clone()),
        Json::Array(items) => AttributeValue::L(
            items
                .iter()
                .map(json_to_attribute)
                .collect::<Result<_, _>>()?,
        ),
        Json::Object(object) if is_tagged(object) => tagged_attribute(object)?,
        Json::Object(object) => AttributeValue::M(json_map_to_item(object)?),
    })
}

pub fn json_to_item(value: &Json) -> Result<Item, DriverError> {
    match value {
        Json::Object(object) => json_map_to_item(object),
        _ => Err(DriverError::query("an item must be a JSON object")),
    }
}

fn json_map_to_item(object: &Map<String, Json>) -> Result<Item, DriverError> {
    object
        .iter()
        .map(|(name, value)| Ok((name.clone(), json_to_attribute(value)?)))
        .collect()
}

fn is_tagged(object: &Map<String, Json>) -> bool {
    object.len() == 1 && object.keys().all(|key| TAGS.contains(&key.as_str()))
}

fn tagged_attribute(object: &Map<String, Json>) -> Result<AttributeValue, DriverError> {
    let Some((tag, value)) = object.iter().next() else {
        return Err(DriverError::internal("tagged value without a tag"));
    };
    match (tag.as_str(), value) {
        (NUMBER_TAG, Json::String(number)) => Ok(AttributeValue::N(number.clone())),
        (BINARY_TAG, Json::String(encoded)) => Ok(AttributeValue::B(decode_blob(encoded)?)),
        (STRING_SET_TAG, Json::Array(items)) => Ok(AttributeValue::Ss(strings(tag, items)?)),
        (NUMBER_SET_TAG, Json::Array(items)) => Ok(AttributeValue::Ns(
            items
                .iter()
                .map(|item| match item {
                    Json::Number(number) => Ok(number.to_string()),
                    Json::String(number) => Ok(number.clone()),
                    _ => Err(invalid_tag(tag)),
                })
                .collect::<Result<_, _>>()?,
        )),
        (BINARY_SET_TAG, Json::Array(items)) => Ok(AttributeValue::Bs(
            strings(tag, items)?
                .iter()
                .map(|encoded| decode_blob(encoded))
                .collect::<Result<_, _>>()?,
        )),
        (MAP_TAG, Json::Object(map)) => Ok(AttributeValue::M(json_map_to_item(map)?)),
        _ => Err(invalid_tag(tag)),
    }
}

fn strings(tag: &str, items: &[Json]) -> Result<Vec<String>, DriverError> {
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid_tag(tag))
        })
        .collect()
}

fn decode_blob(encoded: &str) -> Result<Blob, DriverError> {
    base64::decode(encoded)
        .map(Blob::new)
        .map_err(|_| DriverError::query(format!("\"{encoded}\" is not valid base64")))
}

fn invalid_tag(tag: &str) -> DriverError {
    DriverError::query(format!(
        "invalid {tag} value: use {{\"$N\": \"1.5\"}}, {{\"$B\": \"base64\"}}, \
         {{\"$SS\": [\"a\"]}}, {{\"$NS\": [1]}}, {{\"$BS\": [\"base64\"]}} or {{\"$M\": {{...}}}}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn round_trip(value: AttributeValue) -> AttributeValue {
        let json = attribute_to_json(&value);
        let text = serde_json::to_string(&json).unwrap();
        json_to_attribute(&serde_json::from_str(&text).unwrap()).unwrap()
    }

    #[test]
    fn every_attribute_type_round_trips() {
        let values = [
            AttributeValue::S("text".into()),
            AttributeValue::N("42".into()),
            AttributeValue::N("12345678901234567890.123456789".into()),
            AttributeValue::Bool(true),
            AttributeValue::Null(true),
            AttributeValue::B(Blob::new(vec![0, 1, 255])),
            AttributeValue::Ss(vec!["a".into(), "b".into()]),
            AttributeValue::Ns(vec!["1".into(), "2.5".into()]),
            AttributeValue::Bs(vec![Blob::new(vec![1]), Blob::new(vec![2])]),
            AttributeValue::L(vec![
                AttributeValue::S("x".into()),
                AttributeValue::N("1".into()),
            ]),
            AttributeValue::M(HashMap::from([(
                "$N".to_owned(),
                AttributeValue::S("looks tagged".into()),
            )])),
        ];
        for value in values {
            assert_eq!(round_trip(value.clone()), value);
        }
    }

    #[test]
    fn plain_json_maps_to_natural_types() {
        let item = json_to_item(&json!({
            "id": "user#1",
            "age": 30,
            "score": 1.5,
            "tags": {"$SS": ["a"]},
            "profile": {"city": "Berlin"}
        }))
        .unwrap();
        assert_eq!(item["age"], AttributeValue::N("30".into()));
        assert_eq!(item["score"], AttributeValue::N("1.5".into()));
        assert_eq!(item["tags"], AttributeValue::Ss(vec!["a".into()]));
        assert!(matches!(item["profile"], AttributeValue::M(_)));
        assert_eq!(
            attribute_to_json(&AttributeValue::N("1E+2".into())),
            json!(100.0)
        );
    }

    #[test]
    fn malformed_tags_and_non_objects_are_rejected() {
        assert!(json_to_attribute(&json!({"$B": "***"})).is_err());
        assert!(json_to_attribute(&json!({"$SS": [1]})).is_err());
        assert!(json_to_item(&json!([1])).is_err());
    }
}
