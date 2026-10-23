/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::column_type::{ValueClass, value_class};
use mysql_async::Value as MySqlValue;
use mysql_async::{Column, Params};
use relix_core::Value;

pub fn from_mysql(column: &Column, value: MySqlValue) -> Value {
    if matches!(value, MySqlValue::NULL) {
        return Value::Null;
    }
    match value_class(column) {
        ValueClass::Null => Value::Null,
        ValueClass::SignedInteger => {
            parse_or_fallback(value, |text| text.parse().ok().map(Value::Int))
        }
        ValueClass::UnsignedInteger => {
            parse_or_fallback(value, |text| text.parse().ok().map(Value::UInt))
        }
        ValueClass::Float => parse_or_fallback(value, |text| text.parse().ok().map(Value::Float)),
        ValueClass::Bit => match value {
            MySqlValue::Bytes(bytes) if bytes.len() <= 8 => Value::UInt(big_endian(&bytes)),
            other => generic(other),
        },
        ValueClass::Textual => match value {
            MySqlValue::Bytes(bytes) => Value::Text(String::from_utf8_lossy(&bytes).into_owned()),
            other => generic(other),
        },
        ValueClass::Binary => match value {
            MySqlValue::Bytes(bytes) => Value::Bytes(bytes),
            other => generic(other),
        },
        ValueClass::Character => generic(value),
    }
}

pub fn to_mysql(value: &Value) -> MySqlValue {
    match value {
        Value::Null => MySqlValue::NULL,
        Value::Bool(flag) => MySqlValue::Int(i64::from(*flag)),
        Value::Int(number) => MySqlValue::Int(*number),
        Value::UInt(number) => MySqlValue::UInt(*number),
        Value::Float(number) => MySqlValue::Double(*number),
        Value::Text(text) => MySqlValue::Bytes(text.as_bytes().to_vec()),
        Value::Bytes(bytes) => MySqlValue::Bytes(bytes.clone()),
    }
}

pub fn statement_params(values: Vec<MySqlValue>) -> Params {
    if values.is_empty() {
        Params::Empty
    } else {
        Params::Positional(values)
    }
}

pub fn plain_text(value: &MySqlValue) -> Option<String> {
    match generic(value.clone()) {
        Value::Null => None,
        other => other.to_plain_string(),
    }
}

fn parse_or_fallback(value: MySqlValue, parse: impl Fn(&str) -> Option<Value>) -> Value {
    match value {
        MySqlValue::Bytes(bytes) => std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| parse(text.trim()))
            .unwrap_or_else(|| generic(MySqlValue::Bytes(bytes))),
        other => generic(other),
    }
}

fn generic(value: MySqlValue) -> Value {
    match value {
        MySqlValue::NULL => Value::Null,
        MySqlValue::Bytes(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Value::Text(text),
            Err(error) => Value::Bytes(error.into_bytes()),
        },
        MySqlValue::Int(number) => Value::Int(number),
        MySqlValue::UInt(number) => Value::UInt(number),
        MySqlValue::Float(number) => Value::Float(f64::from(number)),
        MySqlValue::Double(number) => Value::Float(number),
        MySqlValue::Date(year, month, day, hour, minute, second, micros) => {
            Value::Text(format_date(year, month, day, hour, minute, second, micros))
        }
        MySqlValue::Time(negative, days, hours, minutes, seconds, micros) => {
            Value::Text(format_time(negative, days, hours, minutes, seconds, micros))
        }
    }
}

fn big_endian(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte))
}

fn format_date(
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    micros: u32,
) -> String {
    let date = format!("{year:04}-{month:02}-{day:02}");
    if hour == 0 && minute == 0 && second == 0 && micros == 0 {
        return date;
    }
    format!(
        "{date} {}",
        format_clock(u32::from(hour), minute, second, micros)
    )
}

fn format_time(
    negative: bool,
    days: u32,
    hours: u8,
    minutes: u8,
    seconds: u8,
    micros: u32,
) -> String {
    let sign = if negative { "-" } else { "" };
    let total_hours = days * 24 + u32::from(hours);
    format!(
        "{sign}{}",
        format_clock(total_hours, minutes, seconds, micros)
    )
}

fn format_clock(hours: u32, minutes: u8, seconds: u8, micros: u32) -> String {
    let clock = format!("{hours:02}:{minutes:02}:{seconds:02}");
    if micros == 0 {
        clock
    } else {
        format!("{clock}.{micros:06}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column_type::BINARY_CHARSET;
    use mysql_async::consts::{ColumnFlags, ColumnType};

    const UTF8MB4: u16 = 255;

    fn column(column_type: ColumnType, charset: u16, flags: ColumnFlags) -> Column {
        Column::new(column_type)
            .with_character_set(charset)
            .with_flags(flags)
    }

    fn text_bytes(text: &str) -> MySqlValue {
        MySqlValue::Bytes(text.as_bytes().to_vec())
    }

    #[test]
    fn integers_are_parsed_from_text_protocol() {
        let signed = column(
            ColumnType::MYSQL_TYPE_LONG,
            BINARY_CHARSET,
            ColumnFlags::empty(),
        );
        let unsigned = column(
            ColumnType::MYSQL_TYPE_LONGLONG,
            BINARY_CHARSET,
            ColumnFlags::UNSIGNED_FLAG,
        );
        assert_eq!(from_mysql(&signed, text_bytes("-42")), Value::Int(-42));
        assert_eq!(
            from_mysql(&unsigned, text_bytes("18446744073709551615")),
            Value::UInt(u64::MAX)
        );
        assert_eq!(from_mysql(&signed, MySqlValue::Int(7)), Value::Int(7));
    }

    #[test]
    fn floats_and_decimals_convert_by_type() {
        let double = column(
            ColumnType::MYSQL_TYPE_DOUBLE,
            BINARY_CHARSET,
            ColumnFlags::empty(),
        );
        let decimal = column(
            ColumnType::MYSQL_TYPE_NEWDECIMAL,
            BINARY_CHARSET,
            ColumnFlags::empty(),
        );
        assert_eq!(from_mysql(&double, text_bytes("1.5")), Value::Float(1.5));
        assert_eq!(
            from_mysql(&decimal, text_bytes("12.3400")),
            Value::Text("12.3400".into())
        );
    }

    #[test]
    fn binary_charset_yields_bytes_and_text_stays_text() {
        let blob = column(
            ColumnType::MYSQL_TYPE_BLOB,
            BINARY_CHARSET,
            ColumnFlags::BLOB_FLAG,
        );
        let varchar = column(
            ColumnType::MYSQL_TYPE_VAR_STRING,
            UTF8MB4,
            ColumnFlags::empty(),
        );
        assert_eq!(
            from_mysql(&blob, MySqlValue::Bytes(vec![0x61, 0x62])),
            Value::Bytes(vec![0x61, 0x62])
        );
        assert_eq!(
            from_mysql(&varchar, text_bytes("héllo")),
            Value::Text("héllo".into())
        );
        assert_eq!(
            from_mysql(&varchar, MySqlValue::Bytes(vec![0xff, 0xfe])),
            Value::Bytes(vec![0xff, 0xfe])
        );
    }

    #[test]
    fn nulls_bits_and_temporal_values() {
        let int = column(
            ColumnType::MYSQL_TYPE_LONG,
            BINARY_CHARSET,
            ColumnFlags::empty(),
        );
        let bit = column(
            ColumnType::MYSQL_TYPE_BIT,
            BINARY_CHARSET,
            ColumnFlags::UNSIGNED_FLAG,
        );
        let datetime = column(
            ColumnType::MYSQL_TYPE_DATETIME,
            BINARY_CHARSET,
            ColumnFlags::empty(),
        );
        assert_eq!(from_mysql(&int, MySqlValue::NULL), Value::Null);
        assert_eq!(
            from_mysql(&bit, MySqlValue::Bytes(vec![1, 0])),
            Value::UInt(256)
        );
        assert_eq!(
            from_mysql(&datetime, text_bytes("2026-10-23 12:00:00")),
            Value::Text("2026-10-23 12:00:00".into())
        );
        assert_eq!(
            from_mysql(&datetime, MySqlValue::Date(2026, 10, 23, 8, 5, 3, 120)),
            Value::Text("2026-10-23 08:05:03.000120".into())
        );
        assert_eq!(
            generic(MySqlValue::Time(true, 1, 2, 3, 4, 0)),
            Value::Text("-26:03:04".into())
        );
    }

    #[test]
    fn relix_values_bind_as_mysql_values() {
        assert_eq!(to_mysql(&Value::Null), MySqlValue::NULL);
        assert_eq!(to_mysql(&Value::Bool(true)), MySqlValue::Int(1));
        assert_eq!(to_mysql(&Value::UInt(9)), MySqlValue::UInt(9));
        assert_eq!(to_mysql(&Value::Float(0.5)), MySqlValue::Double(0.5));
        assert_eq!(
            to_mysql(&Value::Text("a".into())),
            MySqlValue::Bytes(b"a".to_vec())
        );
    }

    #[test]
    fn plain_text_reads_catalog_values() {
        assert_eq!(plain_text(&text_bytes("users")), Some("users".into()));
        assert_eq!(plain_text(&MySqlValue::UInt(3)), Some("3".into()));
        assert_eq!(plain_text(&MySqlValue::NULL), None);
    }
}
