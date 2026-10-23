/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Value;
use serde_json::value::RawValue;

const WRAPPERS: [&str; 2] = ["Nullable(", "LowCardinality("];
const SMALL_INTEGERS: [&str; 7] = [
    "Int8", "Int16", "Int32", "Int64", "UInt8", "UInt16", "UInt32",
];
const FLOATS: [&str; 3] = ["Float32", "Float64", "BFloat16"];

/// Converts one cell of a `JSONCompactEachRow` row using its ClickHouse type. The raw
/// JSON text is kept for exact numbers (`Decimal`, 128/256-bit integers) and compound
/// values (`Array`, `Map`, `Tuple`, `JSON`), so nothing is rounded through `f64`.
pub(crate) fn from_json(raw: &RawValue, type_name: &str) -> Value {
    let text = raw.get();
    let base = base_type(type_name);
    match text.as_bytes().first() {
        None | Some(b'n') => Value::Null,
        Some(b't') => Value::Bool(true),
        Some(b'f') => Value::Bool(false),
        Some(b'"') => {
            let unquoted = serde_json::from_str::<String>(text).unwrap_or_else(|_| text.to_owned());
            scalar(&unquoted, base).unwrap_or(Value::Text(unquoted))
        }
        Some(b'[' | b'{') => Value::Text(text.to_owned()),
        Some(_) => scalar(text, base).unwrap_or_else(|| Value::Text(text.to_owned())),
    }
}

fn scalar(text: &str, base: &str) -> Option<Value> {
    if SMALL_INTEGERS.contains(&base) {
        return text.parse().ok().map(Value::Int);
    }
    if base == "UInt64" {
        return text.parse().ok().map(Value::UInt);
    }
    if FLOATS.contains(&base) {
        return float(text).map(Value::Float);
    }
    None
}

fn float(text: &str) -> Option<f64> {
    match text {
        "nan" | "-nan" => Some(f64::NAN),
        "inf" | "+inf" => Some(f64::INFINITY),
        "-inf" => Some(f64::NEG_INFINITY),
        other => other.parse().ok(),
    }
}

pub(crate) fn base_type(type_name: &str) -> &str {
    let mut current = type_name.trim();
    while let Some(wrapper) = WRAPPERS
        .iter()
        .find(|wrapper| current.starts_with(**wrapper))
    {
        current = current[wrapper.len()..]
            .strip_suffix(')')
            .unwrap_or(current)
            .trim();
    }
    current
}

pub(crate) fn is_nullable(type_name: &str) -> bool {
    let mut current = type_name.trim();
    while let Some(wrapper) = WRAPPERS
        .iter()
        .find(|wrapper| current.starts_with(**wrapper))
    {
        if *wrapper == "Nullable(" {
            return true;
        }
        current = &current[wrapper.len()..];
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(json: &str, type_name: &str) -> Value {
        let raw: Box<RawValue> = serde_json::from_str(json).unwrap();
        from_json(&raw, type_name)
    }

    #[test]
    fn exact_numbers_keep_their_text() {
        assert_eq!(
            convert("123456789012345678901234.5678", "Decimal(38, 4)"),
            Value::Text("123456789012345678901234.5678".into())
        );
        assert_eq!(
            convert("170141183460469231731687303715884105727", "Int128"),
            Value::Text("170141183460469231731687303715884105727".into())
        );
        assert_eq!(
            convert("\"-9223372036854775808\"", "Int64"),
            Value::Int(i64::MIN)
        );
        assert_eq!(
            convert("18446744073709551615", "UInt64"),
            Value::UInt(u64::MAX)
        );
        assert_eq!(
            convert("7", "LowCardinality(Nullable(UInt8))"),
            Value::Int(7)
        );
    }

    #[test]
    fn floats_strings_and_compound_values() {
        assert_eq!(convert("0.1", "Float32"), Value::Float(0.1));
        assert!(matches!(convert("\"nan\"", "Float64"), Value::Float(value) if value.is_nan()));
        assert_eq!(
            convert("\"-inf\"", "Float64"),
            Value::Float(f64::NEG_INFINITY)
        );
        assert_eq!(convert("null", "Nullable(String)"), Value::Null);
        assert_eq!(convert("\"a\\\"b\"", "String"), Value::Text("a\"b".into()));
        assert_eq!(
            convert("[1,2]", "Array(UInt8)"),
            Value::Text("[1,2]".into())
        );
        assert_eq!(convert("true", "Bool"), Value::Bool(true));
    }

    #[test]
    fn wrappers_are_stripped_for_type_checks() {
        assert_eq!(base_type("LowCardinality(Nullable(String))"), "String");
        assert!(is_nullable("LowCardinality(Nullable(String))"));
        assert!(!is_nullable("Array(Nullable(String))"));
    }
}
