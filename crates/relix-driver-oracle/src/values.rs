/*
 * @author t0bx
 * @since 23.10.2026
 */

use oracle::SqlValue;
use oracle::sql_type::{OracleType, Timestamp};
use relix_core::Value;

const MAX_EXACT_INTEGER_PRECISION: u8 = 18;

/// Converts a fetched Oracle value. Integer columns up to 18 digits become integers;
/// other numbers, temporal values and intervals are text in the session's NLS formats
/// (see the connector setup), so they convert back unchanged when edited.
pub fn from_sql_value(value: &SqlValue) -> Value {
    if value.is_null().unwrap_or(false) {
        return Value::Null;
    }
    let Ok(oracle_type) = value.oracle_type() else {
        return Value::Text(value.to_string());
    };
    let converted = match oracle_type {
        OracleType::Number(precision, 0)
            if (1..=MAX_EXACT_INTEGER_PRECISION).contains(precision) =>
        {
            value.get::<i64>().map(Value::Int)
        }
        OracleType::Int64 => value.get::<i64>().map(Value::Int),
        OracleType::UInt64 => value.get::<u64>().map(Value::UInt),
        OracleType::BinaryFloat | OracleType::BinaryDouble => value.get::<f64>().map(Value::Float),
        OracleType::Boolean => value.get::<bool>().map(Value::Bool),
        OracleType::Raw(_) | OracleType::LongRaw | OracleType::BLOB => {
            value.get::<Vec<u8>>().map(Value::Bytes)
        }
        OracleType::Date => value
            .get::<Timestamp>()
            .map(|timestamp| Value::Text(format_timestamp(&timestamp, 0, false))),
        OracleType::Timestamp(precision) | OracleType::TimestampLTZ(precision) => value
            .get::<Timestamp>()
            .map(|timestamp| Value::Text(format_timestamp(&timestamp, *precision, false))),
        OracleType::TimestampTZ(precision) => value
            .get::<Timestamp>()
            .map(|timestamp| Value::Text(format_timestamp(&timestamp, *precision, true))),
        _ => value.get::<String>().map(Value::Text),
    };
    converted.unwrap_or_else(|_| Value::Text(value.to_string()))
}

pub fn format_timestamp(timestamp: &Timestamp, precision: u8, with_offset: bool) -> String {
    let mut text = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        timestamp.year(),
        timestamp.month(),
        timestamp.day(),
        timestamp.hour(),
        timestamp.minute(),
        timestamp.second()
    );
    let digits = usize::from(precision.min(9));
    if digits > 0 {
        let nanos = format!("{:09}", timestamp.nanosecond());
        text.push('.');
        text.push_str(&nanos[..digits]);
    }
    if with_offset {
        let hours = timestamp.tz_hour_offset();
        let minutes = timestamp.tz_minute_offset();
        let sign = if hours < 0 || minutes < 0 { '-' } else { '+' };
        text.push_str(&format!(" {sign}{:02}:{:02}", hours.abs(), minutes.abs()));
    }
    text
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn timestamps_use_the_session_formats() {
        let date = Timestamp::new(2026, 10, 23, 8, 5, 3, 0).unwrap();
        assert_eq!(format_timestamp(&date, 0, false), "2026-10-23 08:05:03");
        let precise = Timestamp::new(2026, 10, 23, 8, 5, 3, 120_000_000).unwrap();
        assert_eq!(
            format_timestamp(&precise, 6, false),
            "2026-10-23 08:05:03.120000"
        );
        let zoned = precise.and_tz_hm_offset(-5, -30).unwrap();
        assert_eq!(
            format_timestamp(&zoned, 3, true),
            "2026-10-23 08:05:03.120 -05:30"
        );
    }
}
