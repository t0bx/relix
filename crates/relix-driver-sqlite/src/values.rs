/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Value;
use rusqlite::types::{Value as SqliteValue, ValueRef};

pub(crate) fn from_sqlite(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(number) => Value::Int(number),
        ValueRef::Real(number) => Value::Float(number),
        ValueRef::Text(bytes) => Value::Text(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Blob(bytes) => Value::Bytes(bytes.to_vec()),
    }
}

pub(crate) fn to_sqlite(value: &Value) -> SqliteValue {
    match value {
        Value::Null => SqliteValue::Null,
        Value::Bool(flag) => SqliteValue::Integer(i64::from(*flag)),
        Value::Int(number) => SqliteValue::Integer(*number),
        Value::UInt(number) => i64::try_from(*number).map_or_else(
            |_| SqliteValue::Text(number.to_string()),
            SqliteValue::Integer,
        ),
        Value::Float(number) => SqliteValue::Real(*number),
        Value::Text(text) => SqliteValue::Text(text.clone()),
        Value::Bytes(bytes) => SqliteValue::Blob(bytes.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_values_map_to_relix_values() {
        assert_eq!(from_sqlite(ValueRef::Integer(3)), Value::Int(3));
        assert_eq!(from_sqlite(ValueRef::Text(b"hi")), Value::Text("hi".into()));
        assert_eq!(
            from_sqlite(ValueRef::Blob(&[1, 2])),
            Value::Bytes(vec![1, 2])
        );
        assert_eq!(from_sqlite(ValueRef::Null), Value::Null);
    }

    #[test]
    fn relix_values_map_to_sqlite_values() {
        assert_eq!(to_sqlite(&Value::Bool(true)), SqliteValue::Integer(1));
        assert_eq!(to_sqlite(&Value::UInt(7)), SqliteValue::Integer(7));
        assert_eq!(
            to_sqlite(&Value::UInt(u64::MAX)),
            SqliteValue::Text(u64::MAX.to_string())
        );
        assert_eq!(to_sqlite(&Value::Float(1.5)), SqliteValue::Real(1.5));
    }
}
