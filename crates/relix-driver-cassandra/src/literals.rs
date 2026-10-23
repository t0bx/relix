/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::value::hex_string;
use relix_core::{Dialect, Value};
use relix_driver_api::DriverError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiteralKind {
    Quoted,
    Number,
    Boolean,
    Uuid,
    Blob,
    Duration,
    Verbatim,
}

impl LiteralKind {
    fn of(cql_type: &str) -> Self {
        let base = base_type(cql_type);
        match base.as_str() {
            "ascii" | "text" | "varchar" | "inet" | "date" | "time" | "timestamp" => Self::Quoted,
            "tinyint" | "smallint" | "int" | "bigint" | "varint" | "counter" | "decimal"
            | "float" | "double" => Self::Number,
            "boolean" => Self::Boolean,
            "uuid" | "timeuuid" => Self::Uuid,
            "blob" => Self::Blob,
            "duration" => Self::Duration,
            _ => Self::Verbatim,
        }
    }
}

/// Renders a grid value as a CQL literal for a column of the given CQL type. Scalar
/// values are validated so that nothing but a literal of that type can be produced.
/// Collections, tuples, vectors and user-defined types are taken verbatim as CQL
/// literals (`['a', 'b']`, `{'k': 1}`), which is also how the grid displays them.
pub(crate) fn literal(column: &str, cql_type: &str, value: &Value) -> Result<String, DriverError> {
    if value.is_null() {
        return Ok("null".to_owned());
    }
    let kind = LiteralKind::of(cql_type);
    let rendered = match (kind, value) {
        (LiteralKind::Blob, Value::Bytes(bytes)) => Some(hex_string(bytes)),
        (_, Value::Bytes(_)) => None,
        (LiteralKind::Quoted, other) => Some(Dialect::Cql.quote_literal(&plain(other))),
        (LiteralKind::Number, Value::Float(number)) => Some(float_literal(*number)),
        (LiteralKind::Number, other) => Some(plain(other)).filter(|text| is_number(text)),
        (LiteralKind::Boolean, Value::Bool(flag)) => Some(flag.to_string()),
        (LiteralKind::Boolean, other) => {
            let text = plain(other).trim().to_lowercase();
            matches!(text.as_str(), "true" | "false").then_some(text)
        }
        (LiteralKind::Uuid, other) => Some(plain(other)).filter(|text| is_uuid(text)),
        (LiteralKind::Blob, other) => Some(plain(other)).filter(|text| is_hex_blob(text)),
        (LiteralKind::Duration, other) => Some(plain(other)).filter(|text| is_duration(text)),
        (LiteralKind::Verbatim, Value::Text(text)) => {
            Some(text.trim().to_owned()).filter(|text| !text.is_empty())
        }
        (LiteralKind::Verbatim, other) => Some(plain(other)),
    };
    rendered.ok_or_else(|| {
        DriverError::query(format!(
            "'{}' is not a valid {cql_type} value for column {column}",
            plain(value)
        ))
    })
}

pub(crate) fn is_counter(cql_type: &str) -> bool {
    base_type(cql_type) == "counter"
}

fn base_type(cql_type: &str) -> String {
    let mut text = cql_type.trim().to_lowercase();
    while let Some(inner) = text
        .strip_prefix("frozen<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        text = inner.trim().to_owned();
    }
    text.split('<').next().unwrap_or_default().trim().to_owned()
}

fn plain(value: &Value) -> String {
    value.to_plain_string().unwrap_or_default()
}

fn float_literal(number: f64) -> String {
    if number.is_nan() {
        "NaN".to_owned()
    } else if number.is_infinite() {
        if number > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        }
        .to_owned()
    } else {
        number.to_string()
    }
}

fn is_number(text: &str) -> bool {
    let text = text.trim();
    if matches!(text, "NaN" | "Infinity" | "-Infinity") {
        return true;
    }
    if let Some(inner) = text
        .strip_prefix("blobAsVarint(")
        .or_else(|| text.strip_prefix("blobAsDecimal("))
        .and_then(|rest| rest.strip_suffix(')'))
    {
        return is_hex_blob(inner);
    }
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (unsigned, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |part: &str| part.chars().all(|ch| ch.is_ascii_digit());
    let exponent_valid = exponent.is_none_or(|exponent| {
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        !exponent.is_empty() && digits(exponent)
    });
    !whole.is_empty() && digits(whole) && digits(fraction) && exponent_valid
}

fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(group, length)| {
            group.len() == length && group.chars().all(|ch| ch.is_ascii_hexdigit())
        })
}

fn is_hex_blob(text: &str) -> bool {
    text.strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .is_some_and(|hex| hex.chars().all(|ch| ch.is_ascii_hexdigit()))
}

fn is_duration(text: &str) -> bool {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    !unsigned.is_empty()
        && unsigned
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == 'µ')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_are_typed_and_validated() {
        assert_eq!(
            literal("c", "text", &Value::Text("O'Neil".into())).unwrap(),
            "'O''Neil'"
        );
        assert_eq!(literal("c", "int", &Value::Int(4)).unwrap(), "4");
        assert_eq!(
            literal("c", "decimal", &Value::Text("12.50".into())).unwrap(),
            "12.50"
        );
        assert_eq!(
            literal("c", "double", &Value::Float(f64::INFINITY)).unwrap(),
            "Infinity"
        );
        assert_eq!(
            literal("c", "boolean", &Value::Text("TRUE".into())).unwrap(),
            "true"
        );
        assert_eq!(
            literal("c", "blob", &Value::Bytes(vec![0xab])).unwrap(),
            "0xab"
        );
        assert_eq!(literal("c", "timestamp", &Value::Null).unwrap(), "null");
        let uuid = "123e4567-e89b-12d3-a456-426614174000";
        assert_eq!(
            literal("c", "timeuuid", &Value::Text(uuid.into())).unwrap(),
            uuid
        );
        assert_eq!(
            literal("c", "duration", &Value::Text("1h30m".into())).unwrap(),
            "1h30m"
        );
    }

    #[test]
    fn invalid_scalars_are_rejected_instead_of_injected() {
        for (cql_type, text) in [
            ("int", "1; DROP TABLE x"),
            ("uuid", "abc"),
            ("blob", "0xzz"),
            ("boolean", "yes"),
            ("duration", "1h) OR (1"),
        ] {
            let error = literal("c", cql_type, &Value::Text(text.into())).unwrap_err();
            assert!(error.message.contains("not a valid"), "{cql_type}: {error}");
        }
    }

    #[test]
    fn compound_types_are_written_verbatim() {
        assert_eq!(
            literal(
                "tags",
                "frozen<set<text>>",
                &Value::Text(" {'a','b'} ".into())
            )
            .unwrap(),
            "{'a','b'}"
        );
        assert!(literal("tags", "list<int>", &Value::Text(String::new())).is_err());
        assert!(is_counter("counter"));
        assert!(!is_counter("frozen<list<counter>>"));
    }

    #[test]
    fn numbers_accept_exponents_and_exact_forms() {
        for valid in ["1", "-2.5", "1e10", "3.0E-4", "NaN", "blobAsVarint(0x01ff)"] {
            assert!(is_number(valid), "{valid}");
        }
        for invalid in ["", "-", "1.2.3", "1e", "0x10", "1 OR 1"] {
            assert!(!is_number(invalid), "{invalid}");
        }
    }
}
