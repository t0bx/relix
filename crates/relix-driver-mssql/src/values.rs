/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::temporal::{
    format_date, format_datetime, format_datetime_offset, format_datetime2, format_small_datetime,
    format_time,
};
use relix_core::Value;
use tiberius::ColumnData;
use tiberius::numeric::Numeric;

/// Converts a TDS cell into a RELIX value. Exact numerics, temporal values, GUIDs and XML
/// are kept as text so they round-trip unchanged when edited.
pub fn from_column_data(data: ColumnData<'static>) -> Value {
    match data {
        ColumnData::U8(value) => value.map_or(Value::Null, |value| Value::Int(i64::from(value))),
        ColumnData::I16(value) => value.map_or(Value::Null, |value| Value::Int(i64::from(value))),
        ColumnData::I32(value) => value.map_or(Value::Null, |value| Value::Int(i64::from(value))),
        ColumnData::I64(value) => value.map_or(Value::Null, Value::Int),
        ColumnData::F32(value) => value.map_or(Value::Null, |value| Value::Float(f64::from(value))),
        ColumnData::F64(value) => value.map_or(Value::Null, Value::Float),
        ColumnData::Bit(value) => value.map_or(Value::Null, Value::Bool),
        ColumnData::String(value) => {
            value.map_or(Value::Null, |text| Value::Text(text.into_owned()))
        }
        ColumnData::Guid(value) => value.map_or(Value::Null, |guid| {
            Value::Text(guid.to_string().to_ascii_uppercase())
        }),
        ColumnData::Binary(value) => {
            value.map_or(Value::Null, |bytes| Value::Bytes(bytes.into_owned()))
        }
        ColumnData::Numeric(value) => {
            value.map_or(Value::Null, |number| Value::Text(numeric_text(number)))
        }
        ColumnData::Xml(value) => value.map_or(Value::Null, |xml| Value::Text(xml.to_string())),
        ColumnData::DateTime(value) => text_or_null(value, format_datetime),
        ColumnData::SmallDateTime(value) => text_or_null(value, format_small_datetime),
        ColumnData::Time(value) => text_or_null(value, format_time),
        ColumnData::Date(value) => text_or_null(value, format_date),
        ColumnData::DateTime2(value) => text_or_null(value, format_datetime2),
        ColumnData::DateTimeOffset(value) => text_or_null(value, format_datetime_offset),
    }
}

fn text_or_null<T>(value: Option<T>, format: fn(T) -> String) -> Value {
    value.map_or(Value::Null, |value| Value::Text(format(value)))
}

fn numeric_text(number: Numeric) -> String {
    let value = number.value();
    let scale = usize::from(number.scale());
    let digits = value.unsigned_abs().to_string();
    let sign = if value < 0 { "-" } else { "" };
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (integer, fraction) = padded.split_at(padded.len() - scale);
    format!("{sign}{integer}.{fraction}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn integers_floats_and_bits_keep_their_types() {
        assert_eq!(from_column_data(ColumnData::U8(Some(7))), Value::Int(7));
        assert_eq!(from_column_data(ColumnData::I64(Some(-3))), Value::Int(-3));
        assert_eq!(
            from_column_data(ColumnData::F32(Some(1.5))),
            Value::Float(1.5)
        );
        assert_eq!(
            from_column_data(ColumnData::Bit(Some(true))),
            Value::Bool(true)
        );
        assert_eq!(from_column_data(ColumnData::I32(None)), Value::Null);
    }

    #[test]
    fn text_binary_and_guids() {
        assert_eq!(
            from_column_data(ColumnData::String(Some(Cow::Borrowed("héllo")))),
            Value::Text("héllo".into())
        );
        assert_eq!(
            from_column_data(ColumnData::Binary(Some(Cow::Owned(vec![0, 255])))),
            Value::Bytes(vec![0, 255])
        );
        let guid = tiberius::Uuid::from_u128(0xA1B2_C3D4_0000_0000_0000_0000_0000_00FF);
        assert_eq!(
            from_column_data(ColumnData::Guid(Some(guid))),
            Value::Text("A1B2C3D4-0000-0000-0000-0000000000FF".into())
        );
    }

    #[test]
    fn numerics_are_exact_text() {
        let text = |value, scale| numeric_text(Numeric::new_with_scale(value, scale));
        assert_eq!(text(1234, 2), "12.34");
        assert_eq!(text(-5, 1), "-0.5");
        assert_eq!(text(-5, 3), "-0.005");
        assert_eq!(text(42, 0), "42");
        assert_eq!(text(0, 2), "0.00");
    }
}
