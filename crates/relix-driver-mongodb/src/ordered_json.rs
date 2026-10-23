/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::extended_json::from_json;
use mongodb::bson::{Bson, Document};
use relix_driver_api::DriverError;
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value as Json};
use std::fmt;

const EXTENDED_JSON_KEYS: [&str; 17] = [
    "$oid",
    "$symbol",
    "$numberInt",
    "$numberLong",
    "$numberDouble",
    "$numberDecimal",
    "$binary",
    "$uuid",
    "$code",
    "$scope",
    "$timestamp",
    "$regularExpression",
    "$dbPointer",
    "$date",
    "$maxKey",
    "$minKey",
    "$undefined",
];

/// JSON tree that keeps object fields in source order. MongoDB commands require the
/// command name to be the first field and compound sort or index keys depend on field
/// order, which `serde_json::Value` (sorted maps) would lose.
enum OrderedJson {
    Scalar(Json),
    Array(Vec<OrderedJson>),
    Object(Vec<(String, OrderedJson)>),
}

pub fn parse_document(text: &str) -> Result<Document, DriverError> {
    let parsed: OrderedJson = serde_json::from_str(text).map_err(|error| {
        DriverError::query(format!("invalid JSON command: {error}"))
            .with_position(character_offset(text, error.line(), error.column()))
    })?;
    match parsed.into_bson()? {
        Bson::Document(document) => Ok(document),
        _ => Err(DriverError::query(
            "a MongoDB command must be a JSON object such as {\"ping\": 1}",
        )),
    }
}

impl OrderedJson {
    fn into_bson(self) -> Result<Bson, DriverError> {
        match self {
            Self::Scalar(value) => from_json(&value),
            Self::Array(items) => items
                .into_iter()
                .map(Self::into_bson)
                .collect::<Result<Vec<_>, _>>()
                .map(Bson::Array),
            Self::Object(fields) if is_extended_json_value(&fields) => {
                from_json(&Self::Object(fields).into_unordered())
            }
            Self::Object(fields) => {
                let mut document = Document::new();
                for (name, value) in fields {
                    document.insert(name, value.into_bson()?);
                }
                Ok(Bson::Document(document))
            }
        }
    }

    fn into_unordered(self) -> Json {
        match self {
            Self::Scalar(value) => value,
            Self::Array(items) => {
                Json::Array(items.into_iter().map(Self::into_unordered).collect())
            }
            Self::Object(fields) => Json::Object(
                fields
                    .into_iter()
                    .map(|(name, value)| (name, value.into_unordered()))
                    .collect::<Map<_, _>>(),
            ),
        }
    }
}

fn is_extended_json_value(fields: &[(String, OrderedJson)]) -> bool {
    !fields.is_empty()
        && fields
            .iter()
            .all(|(name, _)| EXTENDED_JSON_KEYS.contains(&name.as_str()))
}

fn character_offset(text: &str, line: usize, column: usize) -> usize {
    let preceding: usize = text
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(|line| line.chars().count())
        .sum();
    preceding + column.saturating_sub(1)
}

impl<'de> Deserialize<'de> for OrderedJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(OrderedJsonVisitor)
    }
}

struct OrderedJsonVisitor;

impl<'de> Visitor<'de> for OrderedJsonVisitor {
    type Value = OrderedJson;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<OrderedJson, E> {
        Ok(OrderedJson::Scalar(Json::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<OrderedJson, E> {
        Ok(OrderedJson::Scalar(Json::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<OrderedJson, E> {
        Ok(OrderedJson::Scalar(Json::from(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<OrderedJson, E> {
        Ok(OrderedJson::Scalar(Json::from(value)))
    }

    fn visit_str<E>(self, value: &str) -> Result<OrderedJson, E> {
        Ok(OrderedJson::Scalar(Json::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<OrderedJson, E> {
        Ok(OrderedJson::Scalar(Json::String(value)))
    }

    fn visit_unit<E>(self) -> Result<OrderedJson, E> {
        Ok(OrderedJson::Scalar(Json::Null))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<OrderedJson, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = sequence.next_element()? {
            items.push(item);
        }
        Ok(OrderedJson::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<OrderedJson, A::Error> {
        let mut fields = Vec::new();
        while let Some((name, value)) = map.next_entry::<String, OrderedJson>()? {
            fields.push((name, value));
        }
        Ok(OrderedJson::Object(fields))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;
    use mongodb::bson::oid::ObjectId;
    use pretty_assertions::assert_eq;

    #[test]
    fn field_order_is_preserved() {
        let command = parse_document(r#"{"find": "users", "sort": {"z": 1, "a": -1}}"#).unwrap();
        let keys: Vec<_> = command.keys().map(String::as_str).collect();
        assert_eq!(keys, ["find", "sort"]);
        let sort = command.get_document("sort").unwrap();
        assert_eq!(sort.keys().collect::<Vec<_>>(), ["z", "a"]);
    }

    #[test]
    fn extended_json_values_are_decoded() {
        let command = parse_document(
            r#"{"find": "users", "filter": {"_id": {"$oid": "65f0a1b2c3d4e5f601234567"},
                "n": {"$numberLong": "5"}, "age": {"$gt": -3}}}"#,
        )
        .unwrap();
        let filter = command.get_document("filter").unwrap();
        assert_eq!(
            filter.get("_id"),
            Some(&Bson::ObjectId(
                ObjectId::parse_str("65f0a1b2c3d4e5f601234567").unwrap()
            ))
        );
        assert_eq!(filter.get("n"), Some(&Bson::Int64(5)));
        assert_eq!(filter.get_document("age").unwrap(), &doc! { "$gt": -3_i32 });
    }

    #[test]
    fn errors_report_the_character_position() {
        let error = parse_document("{\"ping\": 1,\n  oops}").unwrap_err();
        assert_eq!(error.position, Some(14));
        assert!(parse_document("[1, 2]").is_err());
    }
}
