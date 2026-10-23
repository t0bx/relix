/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::models::{TableField, TableRow};
use crate::field_types::type_label;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::DateTime;
use relix_core::{ColumnMeta, Value};
use serde_json::Value as Json;

const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.f UTC";

pub(crate) fn column_meta(field: &TableField) -> ColumnMeta {
    ColumnMeta::new(field.name.clone(), Some(type_label(field)))
}

pub(crate) fn row_values(row: &TableRow, fields: &[TableField]) -> Vec<Value> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let cell = row.f.get(index).map_or(&Json::Null, |cell| &cell.v);
            cell_value(cell, field)
        })
        .collect()
}

/// Converts one cell of the REST row format (`{"f": [{"v": …}]}`). Scalars arrive as
/// strings; arrays and structs are rendered as JSON text so they stay readable in the
/// grid. Exact numerics, dates and geography values remain text.
pub(crate) fn cell_value(cell: &Json, field: &TableField) -> Value {
    if cell.is_null() {
        return Value::Null;
    }
    if field.is_repeated() || field.is_record() {
        return Value::Text(nested_json(cell, field).to_string());
    }
    let Some(raw) = cell.as_str() else {
        return Value::Text(cell.to_string());
    };
    let converted = match field.field_type.to_ascii_uppercase().as_str() {
        "INTEGER" | "INT64" => raw.parse().ok().map(Value::Int),
        "FLOAT" | "FLOAT64" => raw.parse().ok().map(Value::Float),
        "BOOLEAN" | "BOOL" => raw.parse().ok().map(Value::Bool),
        "BYTES" => STANDARD.decode(raw).ok().map(Value::Bytes),
        "TIMESTAMP" => timestamp(raw).map(Value::Text),
        _ => None,
    };
    converted.unwrap_or_else(|| Value::Text(raw.to_owned()))
}

fn nested_json(cell: &Json, field: &TableField) -> Json {
    if field.is_repeated() {
        let element = TableField {
            mode: None,
            ..field.clone()
        };
        let items = cell.as_array().map(Vec::as_slice).unwrap_or_default();
        return Json::Array(
            items
                .iter()
                .map(|item| nested_json(item.get("v").unwrap_or(&Json::Null), &element))
                .collect(),
        );
    }
    if field.is_record() {
        let cells = cell
            .get("f")
            .and_then(Json::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let members = field.fields.iter().enumerate().map(|(index, member)| {
            let value = cells
                .get(index)
                .and_then(|cell| cell.get("v"))
                .unwrap_or(&Json::Null);
            (member.name.clone(), nested_json(value, member))
        });
        return Json::Object(members.collect());
    }
    if cell.is_null() {
        return Json::Null;
    }
    match cell_value(cell, field) {
        Value::Bytes(_) => cell.clone(),
        other => other.to_json(),
    }
}

/// Timestamps are requested as int64 microseconds; older servers and emulators may
/// still send floating-point seconds, which are accepted as well.
fn timestamp(raw: &str) -> Option<String> {
    let micros = match raw.parse::<i64>() {
        Ok(micros) => micros,
        Err(_) => {
            let seconds: f64 = raw.parse().ok()?;
            let micros = (seconds * 1_000_000.0).round();
            if !micros.is_finite() || micros.abs() > i64::MAX as f64 {
                return None;
            }
            micros as i64
        }
    };
    DateTime::from_timestamp_micros(micros)
        .map(|instant| instant.format(TIMESTAMP_FORMAT).to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use serde_json::json;

    fn field(
        name: &str,
        field_type: &str,
        mode: Option<&str>,
        fields: Vec<TableField>,
    ) -> TableField {
        TableField {
            name: name.into(),
            field_type: field_type.into(),
            mode: mode.map(str::to_owned),
            fields,
            ..TableField::default()
        }
    }

    #[test]
    fn scalars_follow_their_types() {
        let row: TableRow = serde_json::from_value(json!({"f": [
            {"v": "7"}, {"v": "2.5"}, {"v": "true"}, {"v": "Af8="}, {"v": "12.50"},
            {"v": "1704164645123456"}, {"v": "1.7041646451E9"}, {"v": null}
        ]}))
        .unwrap();
        let fields = vec![
            field("i", "INTEGER", None, vec![]),
            field("f", "FLOAT", None, vec![]),
            field("b", "BOOLEAN", None, vec![]),
            field("y", "BYTES", None, vec![]),
            field("n", "NUMERIC", None, vec![]),
            field("t", "TIMESTAMP", None, vec![]),
            field("legacy", "TIMESTAMP", None, vec![]),
            field("s", "STRING", None, vec![]),
        ];
        assert_eq!(
            row_values(&row, &fields),
            vec![
                Value::Int(7),
                Value::Float(2.5),
                Value::Bool(true),
                Value::Bytes(vec![1, 255]),
                Value::Text("12.50".into()),
                Value::Text("2024-01-02 03:04:05.123456 UTC".into()),
                Value::Text("2024-01-02 03:04:05.100 UTC".into()),
                Value::Null,
            ]
        );
    }

    #[test]
    fn arrays_and_structs_render_as_json() {
        let tags = field("tags", "STRING", Some("REPEATED"), vec![]);
        assert_eq!(
            cell_value(&json!([{"v": "a"}, {"v": "b"}]), &tags),
            Value::Text(r#"["a","b"]"#.into())
        );
        let meta = field(
            "meta",
            "RECORD",
            None,
            vec![
                field("a", "INTEGER", None, vec![]),
                field("b", "STRING", None, vec![]),
                field("ids", "INTEGER", Some("REPEATED"), vec![]),
            ],
        );
        assert_eq!(
            cell_value(
                &json!({"f": [{"v": "1"}, {"v": null}, {"v": [{"v": "4"}]}]}),
                &meta
            ),
            Value::Text(r#"{"a":1,"b":null,"ids":[4]}"#.into())
        );
    }
}
