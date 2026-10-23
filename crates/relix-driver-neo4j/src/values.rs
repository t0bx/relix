/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::temporal::temporal_text;
use neo4rs::{BoltMap, BoltNode, BoltRelation, BoltType, BoltUnboundedRelation};
use relix_core::Value;
use relix_core::value::hex_string;
use serde_json::{Map, Value as Json, json};

/// Converts a Bolt value into a grid value. Scalars keep their type and temporal values
/// become ISO 8601 text. Lists, maps, points and graph values (nodes, relationships and
/// paths) are rendered as compact JSON so the grid and the JSON viewer can show them;
/// graph entities carry their labels or type, their id and their properties.
pub(crate) fn to_value(value: &BoltType) -> Value {
    match value {
        BoltType::Null(_) => Value::Null,
        BoltType::Boolean(flag) => Value::Bool(flag.value),
        BoltType::Integer(number) => Value::Int(number.value),
        BoltType::Float(number) => Value::Float(number.value),
        BoltType::String(text) => Value::Text(text.value.clone()),
        BoltType::Bytes(bytes) => Value::Bytes(bytes.value.to_vec()),
        other => match temporal_text(other) {
            Some(text) => Value::Text(text),
            None => Value::Text(to_json(other).to_string()),
        },
    }
}

pub(crate) fn to_json(value: &BoltType) -> Json {
    match value {
        BoltType::Null(_) => Json::Null,
        BoltType::Boolean(flag) => Json::Bool(flag.value),
        BoltType::Integer(number) => json!(number.value),
        BoltType::Float(number) => serde_json::Number::from_f64(number.value)
            .map(Json::Number)
            .unwrap_or_else(|| Json::String(number.value.to_string())),
        BoltType::String(text) => Json::String(text.value.clone()),
        BoltType::Bytes(bytes) => Json::String(hex_string(&bytes.value)),
        BoltType::List(list) => Json::Array(list.value.iter().map(to_json).collect()),
        BoltType::Map(map) => map_json(map),
        BoltType::Node(node) => node_json(node),
        BoltType::Relation(relation) => relation_json(relation),
        BoltType::UnboundedRelation(relation) => unbounded_relation_json(relation),
        BoltType::Path(path) => json!({
            "nodes": path.nodes().iter().map(node_json).collect::<Vec<_>>(),
            "relationships": path.rels().iter().map(unbounded_relation_json).collect::<Vec<_>>(),
        }),
        BoltType::Point2D(point) => json!({
            "srid": point.sr_id.value,
            "x": point.x.value,
            "y": point.y.value,
        }),
        BoltType::Point3D(point) => json!({
            "srid": point.sr_id.value,
            "x": point.x.value,
            "y": point.y.value,
            "z": point.z.value,
        }),
        other => temporal_text(other).map_or(Json::Null, Json::String),
    }
}

pub(crate) fn type_label(value: &BoltType) -> &'static str {
    match value {
        BoltType::Null(_) => "Null",
        BoltType::Boolean(_) => "Boolean",
        BoltType::Integer(_) => "Integer",
        BoltType::Float(_) => "Float",
        BoltType::String(_) => "String",
        BoltType::Bytes(_) => "ByteArray",
        BoltType::List(_) => "List",
        BoltType::Map(_) => "Map",
        BoltType::Node(_) => "Node",
        BoltType::Relation(_) | BoltType::UnboundedRelation(_) => "Relationship",
        BoltType::Path(_) => "Path",
        BoltType::Point2D(_) | BoltType::Point3D(_) => "Point",
        BoltType::Date(_) => "Date",
        BoltType::Time(_) => "Time",
        BoltType::LocalTime(_) => "LocalTime",
        BoltType::DateTime(_) | BoltType::DateTimeZoneId(_) => "DateTime",
        BoltType::LocalDateTime(_) => "LocalDateTime",
        BoltType::Duration(_) => "Duration",
    }
}

fn map_json(map: &BoltMap) -> Json {
    let mut entries: Vec<(&String, Json)> = map
        .value
        .iter()
        .map(|(key, value)| (&key.value, to_json(value)))
        .collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.clone(), value))
            .collect::<Map<String, Json>>(),
    )
}

fn node_json(node: &BoltNode) -> Json {
    let labels: Vec<Json> = node.labels.value.iter().map(to_json).collect();
    json!({
        "id": node.id.value,
        "labels": labels,
        "properties": map_json(&node.properties),
    })
}

fn relation_json(relation: &BoltRelation) -> Json {
    json!({
        "id": relation.id.value,
        "type": relation.typ.value,
        "start": relation.start_node_id.value,
        "end": relation.end_node_id.value,
        "properties": map_json(&relation.properties),
    })
}

fn unbounded_relation_json(relation: &BoltUnboundedRelation) -> Json {
    json!({
        "id": relation.id.value,
        "type": relation.typ.value,
        "properties": map_json(&relation.properties),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo4rs::{BoltInteger, BoltList, BoltString};

    fn properties() -> BoltMap {
        let mut map = BoltMap::new();
        map.put(BoltString::new("name"), BoltType::from("Ada"));
        map.put(BoltString::new("age"), BoltType::from(36_i64));
        map
    }

    #[test]
    fn scalars_keep_their_types() {
        assert_eq!(to_value(&BoltType::from(4_i64)), Value::Int(4));
        assert_eq!(to_value(&BoltType::from(true)), Value::Bool(true));
        assert_eq!(to_value(&BoltType::from("x")), Value::Text("x".into()));
        assert_eq!(to_value(&BoltType::from(Option::<i64>::None)), Value::Null);
    }

    #[test]
    fn nodes_render_as_compact_json() {
        let mut labels = BoltList::new();
        labels.push(BoltType::from("Person"));
        let node = BoltNode::new(BoltInteger::new(7), labels, properties());
        assert_eq!(
            to_value(&BoltType::Node(node)),
            Value::Text(
                r#"{"id":7,"labels":["Person"],"properties":{"age":36,"name":"Ada"}}"#.into()
            )
        );
    }

    #[test]
    fn relationships_and_lists_render_as_json() {
        let relation = BoltRelation {
            id: BoltInteger::new(1),
            start_node_id: BoltInteger::new(2),
            end_node_id: BoltInteger::new(3),
            typ: BoltString::new("KNOWS"),
            properties: BoltMap::new(),
        };
        assert_eq!(
            to_json(&BoltType::Relation(relation)),
            json!({"id": 1, "type": "KNOWS", "start": 2, "end": 3, "properties": {}})
        );
        let mut list = BoltList::new();
        list.push(BoltType::from(1_i64));
        list.push(BoltType::from("a"));
        assert_eq!(
            to_value(&BoltType::List(list)),
            Value::Text(r#"[1,"a"]"#.into())
        );
    }
}
