/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Value;

pub(crate) fn typed_value(type_name: Option<&str>, text: Option<&str>) -> Value {
    let Some(text) = text else {
        return Value::Null;
    };
    let typed = match type_name {
        Some("bool") => match text {
            "t" | "true" => Some(Value::Bool(true)),
            "f" | "false" => Some(Value::Bool(false)),
            _ => None,
        },
        Some("int2" | "int4" | "int8" | "oid") => text.parse().ok().map(Value::Int),
        Some("float4" | "float8") => text.parse().ok().map(Value::Float),
        _ => None,
    };
    typed.unwrap_or_else(|| Value::Text(text.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_stays_null_for_every_type() {
        assert_eq!(typed_value(Some("int4"), None), Value::Null);
        assert_eq!(typed_value(None, None), Value::Null);
    }

    #[test]
    fn scalar_types_are_converted() {
        assert_eq!(typed_value(Some("bool"), Some("t")), Value::Bool(true));
        assert_eq!(typed_value(Some("bool"), Some("f")), Value::Bool(false));
        assert_eq!(typed_value(Some("int8"), Some("-42")), Value::Int(-42));
        assert_eq!(
            typed_value(Some("oid"), Some("4294967295")),
            Value::Int(4_294_967_295)
        );
        assert_eq!(typed_value(Some("float8"), Some("1.5")), Value::Float(1.5));
        assert_eq!(
            typed_value(Some("float4"), Some("-Infinity")),
            Value::Float(f64::NEG_INFINITY)
        );
        assert!(
            matches!(typed_value(Some("float8"), Some("NaN")), Value::Float(value) if value.is_nan())
        );
    }

    #[test]
    fn other_types_stay_textual() {
        assert_eq!(
            typed_value(Some("numeric"), Some("12.50")),
            Value::Text("12.50".into())
        );
        assert_eq!(
            typed_value(Some("jsonb"), Some("{\"a\": 1}")),
            Value::Text("{\"a\": 1}".into())
        );
        assert_eq!(typed_value(None, Some("7")), Value::Text("7".into()));
    }
}
