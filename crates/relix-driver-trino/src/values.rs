/*
 * @author t0bx
 * @since 23.10.2026
 */

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use relix_core::Value;
use serde_json::value::RawValue;

const INTEGER_TYPES: [&str; 4] = ["tinyint", "smallint", "integer", "bigint"];
const FLOAT_TYPES: [&str; 2] = ["real", "double"];
const BINARY_TYPE: &str = "varbinary";

/// Converts one cell of a result page using its Trino type. Integers become numbers,
/// `varbinary` (base64 on the wire) becomes bytes, decimals and temporal values keep
/// their exact text, and arrays, maps and rows keep their JSON text.
pub(crate) fn from_json(raw: &RawValue, type_name: &str) -> Value {
    let text = raw.get();
    let base = base_type(type_name);
    match text.as_bytes().first() {
        None | Some(b'n') => Value::Null,
        Some(b't') => Value::Bool(true),
        Some(b'f') => Value::Bool(false),
        Some(b'[' | b'{') => Value::Text(text.to_owned()),
        Some(b'"') => {
            let unquoted = serde_json::from_str::<String>(text).unwrap_or_else(|_| text.to_owned());
            string_value(unquoted, base)
        }
        Some(_) => number_value(text, base),
    }
}

fn string_value(text: String, base: &str) -> Value {
    if base == BINARY_TYPE {
        return STANDARD
            .decode(text.as_bytes())
            .map_or(Value::Text(text), Value::Bytes);
    }
    if FLOAT_TYPES.contains(&base) {
        let special = match text.as_str() {
            "NaN" => Some(f64::NAN),
            "Infinity" => Some(f64::INFINITY),
            "-Infinity" => Some(f64::NEG_INFINITY),
            _ => None,
        };
        if let Some(number) = special {
            return Value::Float(number);
        }
    }
    Value::Text(text)
}

fn number_value(text: &str, base: &str) -> Value {
    if INTEGER_TYPES.contains(&base)
        && let Ok(number) = text.parse()
    {
        return Value::Int(number);
    }
    if FLOAT_TYPES.contains(&base)
        && let Ok(number) = text.parse()
    {
        return Value::Float(number);
    }
    Value::Text(text.to_owned())
}

fn base_type(type_name: &str) -> &str {
    type_name
        .split_once('(')
        .map_or(type_name, |(base, _)| base)
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(json: &str, type_name: &str) -> Value {
        let raw: Box<RawValue> = serde_json::from_str(json).unwrap();
        from_json(&raw, type_name)
    }

    #[test]
    fn numbers_keep_their_precision() {
        assert_eq!(
            convert("9223372036854775807", "bigint"),
            Value::Int(i64::MAX)
        );
        assert_eq!(convert("1.5", "double"), Value::Float(1.5));
        assert_eq!(
            convert("\"12345678901234567890.12\"", "decimal(22,2)"),
            Value::Text("12345678901234567890.12".into())
        );
        assert!(matches!(convert("\"NaN\"", "double"), Value::Float(value) if value.is_nan()));
    }

    #[test]
    fn binary_strings_and_structures() {
        assert_eq!(convert("\"AP8=\"", "varbinary"), Value::Bytes(vec![0, 255]));
        assert_eq!(convert("\"x\"", "varchar(3)"), Value::Text("x".into()));
        assert_eq!(
            convert("[1,{\"a\":null}]", "array(map(varchar, integer))"),
            Value::Text("[1,{\"a\":null}]".into())
        );
        assert_eq!(convert("null", "integer"), Value::Null);
        assert_eq!(convert("true", "boolean"), Value::Bool(true));
    }
}
