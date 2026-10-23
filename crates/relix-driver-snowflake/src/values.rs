/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::models::RowType;
use crate::temporal::{TimestampZone, date_from_days, time_of_day, timestamp};
use relix_core::{ColumnMeta, Value};

const DEFAULT_FRACTION_DIGITS: u32 = 9;

/// Cells of the `jsonv2` result format are strings or `null`; other JSON values are
/// accepted defensively and kept in their textual form.
pub(crate) fn cell_text(cell: &serde_json::Value) -> Option<String> {
    match cell {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

/// Converts a `jsonv2` cell according to its column type. Exact decimals, semi-structured
/// values and anything that does not parse stay text so they round-trip unchanged.
pub(crate) fn convert(raw: Option<&str>, column: &RowType) -> Value {
    let Some(raw) = raw else {
        return Value::Null;
    };
    let scale = column.scale.unwrap_or(DEFAULT_FRACTION_DIGITS);
    let converted = match column.type_name.to_ascii_lowercase().as_str() {
        "fixed" if column.scale.unwrap_or(0) == 0 => raw.parse().ok().map(Value::Int),
        "real" => raw.parse().ok().map(Value::Float),
        "boolean" => boolean(raw).map(Value::Bool),
        "binary" => decode_hex(raw).map(Value::Bytes),
        "date" => date_from_days(raw).map(Value::Text),
        "time" => time_of_day(raw, scale).map(Value::Text),
        "timestamp_ntz" => timestamp(raw, scale, TimestampZone::Naive).map(Value::Text),
        "timestamp_ltz" => timestamp(raw, scale, TimestampZone::Utc).map(Value::Text),
        "timestamp_tz" => timestamp(raw, scale, TimestampZone::Encoded).map(Value::Text),
        _ => None,
    };
    converted.unwrap_or_else(|| Value::Text(raw.to_owned()))
}

pub(crate) fn column_meta(column: &RowType) -> ColumnMeta {
    ColumnMeta::new(column.name.clone(), Some(type_label(column)))
}

fn type_label(column: &RowType) -> String {
    match column.type_name.to_ascii_lowercase().as_str() {
        "fixed" => format!(
            "NUMBER({},{})",
            column.precision.unwrap_or(38),
            column.scale.unwrap_or(0)
        ),
        "real" => "FLOAT".to_owned(),
        "text" => "VARCHAR".to_owned(),
        other => other.to_uppercase(),
    }
}

fn boolean(raw: &str) -> Option<bool> {
    match raw.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn decode_hex(raw: &str) -> Option<Vec<u8>> {
    if !raw.len().is_multiple_of(2) {
        return None;
    }
    (0..raw.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(raw.get(index..index + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(type_name: &str, scale: Option<u32>) -> RowType {
        RowType {
            name: "C".into(),
            type_name: type_name.into(),
            scale,
            precision: Some(38),
            nullable: true,
        }
    }

    #[test]
    fn numbers_keep_exact_decimals_as_text() {
        assert_eq!(
            convert(Some("42"), &column("fixed", Some(0))),
            Value::Int(42)
        );
        assert_eq!(
            convert(Some("99999999999999999999"), &column("fixed", Some(0))),
            Value::Text("99999999999999999999".into())
        );
        assert_eq!(
            convert(Some("1.50"), &column("fixed", Some(2))),
            Value::Text("1.50".into())
        );
        assert_eq!(
            convert(Some("2.5"), &column("real", None)),
            Value::Float(2.5)
        );
    }

    #[test]
    fn typed_cells_are_decoded() {
        assert_eq!(convert(None, &column("text", None)), Value::Null);
        assert_eq!(
            convert(Some("true"), &column("boolean", None)),
            Value::Bool(true)
        );
        assert_eq!(
            convert(Some("0aff"), &column("binary", None)),
            Value::Bytes(vec![10, 255])
        );
        assert_eq!(
            convert(Some("18262"), &column("date", None)),
            Value::Text("2020-01-01".into())
        );
        assert_eq!(
            convert(Some("{\n  \"a\": 1\n}"), &column("variant", None)),
            Value::Text("{\n  \"a\": 1\n}".into())
        );
    }

    #[test]
    fn column_types_are_labelled() {
        assert_eq!(
            column_meta(&column("fixed", Some(2))).type_name.as_deref(),
            Some("NUMBER(38,2)")
        );
        assert_eq!(
            column_meta(&column("timestamp_tz", Some(9)))
                .type_name
                .as_deref(),
            Some("TIMESTAMP_TZ")
        );
        assert_eq!(cell_text(&serde_json::json!(5)).as_deref(), Some("5"));
    }
}
