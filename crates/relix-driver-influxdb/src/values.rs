/*
 * @author t0bx
 * @since 24.10.2026
 */

use relix_core::Value;
use serde_json::Value as Json;

/// Converts a JSON Lines cell into a grid value. InfluxDB renders timestamps as UTC
/// without an offset (`2024-05-06T07:08:09.123456789`); `timestamp` marks columns known
/// to hold timestamps, whose values get the `Z` suffix that makes them RFC 3339.
pub(crate) fn to_value(cell: Option<&Json>, timestamp: bool) -> Value {
    match cell {
        None | Some(Json::Null) => Value::Null,
        Some(Json::Bool(flag)) => Value::Bool(*flag),
        Some(Json::Number(number)) => match (number.as_i64(), number.as_u64()) {
            (Some(signed), _) => Value::Int(signed),
            (None, Some(unsigned)) => Value::UInt(unsigned),
            _ => Value::Float(number.as_f64().unwrap_or(f64::NAN)),
        },
        Some(Json::String(text)) if timestamp && is_server_timestamp(text) => {
            Value::Text(format!("{text}Z"))
        }
        Some(Json::String(text)) => Value::Text(text.clone()),
        Some(other) => Value::Text(other.to_string()),
    }
}

/// Recognises the server's timestamp rendering: `YYYY-MM-DDTHH:MM:SS` with an optional
/// fraction and no offset.
pub(crate) fn is_server_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() < 19 {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    let shape = digits(0..4)
        && bytes[4] == b'-'
        && digits(5..7)
        && bytes[7] == b'-'
        && digits(8..10)
        && bytes[10] == b'T'
        && digits(11..13)
        && bytes[13] == b':'
        && digits(14..16)
        && bytes[16] == b':'
        && digits(17..19);
    let fraction = match &bytes[19..] {
        [] => true,
        [b'.', rest @ ..] => !rest.is_empty() && rest.iter().all(u8::is_ascii_digit),
        _ => false,
    };
    shape && fraction
}

pub(crate) fn is_timestamp_type(data_type: &str) -> bool {
    data_type.starts_with("Timestamp")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scalars_keep_their_types() {
        assert_eq!(to_value(Some(&json!(3)), false), Value::Int(3));
        assert_eq!(
            to_value(Some(&json!(u64::MAX)), false),
            Value::UInt(u64::MAX)
        );
        assert_eq!(to_value(Some(&json!(1.5)), false), Value::Float(1.5));
        assert_eq!(to_value(Some(&json!(true)), false), Value::Bool(true));
        assert_eq!(to_value(None, false), Value::Null);
    }

    #[test]
    fn timestamps_become_rfc3339() {
        assert_eq!(
            to_value(Some(&json!("2023-11-14T22:13:22.123456789")), true),
            Value::Text("2023-11-14T22:13:22.123456789Z".into())
        );
        assert_eq!(
            to_value(Some(&json!("2023-11-14T22:13:22")), false),
            Value::Text("2023-11-14T22:13:22".into())
        );
        assert!(!is_server_timestamp("2023-11-14T22:13:22Z"));
        assert!(!is_server_timestamp("2023-11-14 22:13:22"));
        assert!(is_timestamp_type("Timestamp(ns)"));
    }
}
