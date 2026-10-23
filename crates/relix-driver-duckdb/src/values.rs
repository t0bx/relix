/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::binary_text::{bignum_text, bit_string};
use crate::nested_text::render_nested;
use crate::temporal::{format_date, format_interval, format_time, format_timestamp};
use crate::type_names::ColumnType;
use duckdb::core::LogicalTypeId;
use duckdb::types::{ToSqlOutput, Value as DuckValue, ValueRef};
use relix_core::Value;

/// Converts a top-level result cell. Exact numbers that do not fit the client types
/// (`HUGEINT`, `DECIMAL`), temporal values and nested values become DuckDB's own text
/// form so they round-trip through edits unchanged.
pub(crate) fn from_duckdb(value: ValueRef<'_>, column: &ColumnType) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Boolean(flag) => Value::Bool(flag),
        ValueRef::TinyInt(number) => Value::Int(i64::from(number)),
        ValueRef::SmallInt(number) => Value::Int(i64::from(number)),
        ValueRef::Int(number) => Value::Int(i64::from(number)),
        ValueRef::BigInt(number) => Value::Int(number),
        ValueRef::UTinyInt(number) => Value::Int(i64::from(number)),
        ValueRef::USmallInt(number) => Value::Int(i64::from(number)),
        ValueRef::UInt(number) => Value::Int(i64::from(number)),
        ValueRef::UBigInt(number) => Value::UInt(number),
        ValueRef::HugeInt(number) => {
            i64::try_from(number).map_or_else(|_| Value::Text(number.to_string()), Value::Int)
        }
        ValueRef::UHugeInt(number) => {
            u64::try_from(number).map_or_else(|_| Value::Text(number.to_string()), Value::UInt)
        }
        ValueRef::Float(number) => Value::Float(shortest_f32(number)),
        ValueRef::Double(number) => Value::Float(number),
        ValueRef::Decimal(decimal) => Value::Text(decimal.to_string()),
        ValueRef::Timestamp(unit, ticks) => {
            Value::Text(format_timestamp(unit, ticks, column.has_time_zone()))
        }
        ValueRef::Date32(days) => Value::Text(format_date(days)),
        ValueRef::Time64(unit, ticks) => Value::Text(format_time(unit, ticks)),
        ValueRef::Interval {
            months,
            days,
            nanos,
        } => Value::Text(format_interval(months, days, nanos)),
        ValueRef::Text(bytes) => Value::Text(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Blob(bytes) => match column.id {
            LogicalTypeId::Bit => Value::Text(bit_string(bytes)),
            LogicalTypeId::Bignum => {
                bignum_text(bytes).map_or_else(|| Value::Bytes(bytes.to_vec()), Value::Text)
            }
            _ => Value::Bytes(bytes.to_vec()),
        },
        ValueRef::Geometry(bytes) => Value::Bytes(bytes.to_vec()),
        nested => Value::Text(render_nested(&nested.to_owned(), Some(column))),
    }
}

/// Converts a grid value to a statement parameter. Floats are bound in their shortest
/// text form so DuckDB casts them to the column's own type: a `FLOAT` cell compared as
/// `DOUBLE` would never equal the value it was read from.
pub(crate) fn to_duckdb(value: &Value) -> ToSqlOutput<'_> {
    match value {
        Value::Null => ToSqlOutput::Owned(DuckValue::Null),
        Value::Bool(flag) => ToSqlOutput::Owned(DuckValue::Boolean(*flag)),
        Value::Int(number) => ToSqlOutput::Owned(DuckValue::BigInt(*number)),
        Value::UInt(number) => ToSqlOutput::Owned(DuckValue::UBigInt(*number)),
        Value::Float(number) => ToSqlOutput::Owned(DuckValue::Text(number.to_string())),
        Value::Text(text) => ToSqlOutput::Borrowed(ValueRef::Text(text.as_bytes())),
        Value::Bytes(bytes) => ToSqlOutput::Borrowed(ValueRef::Blob(bytes)),
    }
}

/// Widens a `FLOAT` through its shortest decimal form, so `0.1::FLOAT` shows as `0.1`
/// instead of `0.10000000149011612`.
fn shortest_f32(number: f32) -> f64 {
    number.to_string().parse().unwrap_or(f64::from(number))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_keep_their_shortest_representation() {
        assert_eq!(shortest_f32(0.1), 0.1);
        assert!(shortest_f32(f32::NAN).is_nan());
    }
}
