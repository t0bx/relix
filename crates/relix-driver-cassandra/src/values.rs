/*
 * @author t0bx
 * @since 23.10.2026
 */

use chrono::{DateTime, Days, NaiveDate};
use relix_core::Value;
use scylla::value::{CqlDate, CqlDecimal, CqlDuration, CqlTime, CqlValue, CqlVarint};

const DATE_EPOCH_OFFSET: i64 = 1 << 31;
const NANOS_PER_SECOND: i64 = 1_000_000_000;

/// Converts a CQL value into a grid value. Scalars keep their type; exact numbers,
/// temporal values and identifiers become text in the literal syntax CQL accepts, and
/// collections, tuples, vectors and user-defined types are rendered as CQL literals so
/// that an edited cell can be written back verbatim.
pub(crate) fn to_value(value: Option<CqlValue>) -> Value {
    let Some(value) = value else {
        return Value::Null;
    };
    match value {
        CqlValue::Ascii(text) | CqlValue::Text(text) => Value::Text(text),
        CqlValue::Boolean(flag) => Value::Bool(flag),
        CqlValue::Blob(bytes) => Value::Bytes(bytes),
        CqlValue::Empty => Value::Bytes(Vec::new()),
        CqlValue::Counter(counter) => Value::Int(counter.0),
        CqlValue::BigInt(number) => Value::Int(number),
        CqlValue::Int(number) => Value::Int(i64::from(number)),
        CqlValue::SmallInt(number) => Value::Int(i64::from(number)),
        CqlValue::TinyInt(number) => Value::Int(i64::from(number)),
        CqlValue::Double(number) => Value::Float(number),
        CqlValue::Float(number) => Value::Float(widen_float(number)),
        CqlValue::Decimal(decimal) => Value::Text(decimal_text(&decimal)),
        CqlValue::Varint(varint) => Value::Text(varint_text(&varint)),
        CqlValue::Timestamp(timestamp) => Value::Text(timestamp_text(timestamp.0)),
        CqlValue::Date(date) => Value::Text(date_text(date)),
        CqlValue::Time(time) => Value::Text(time_text(time)),
        CqlValue::Duration(duration) => Value::Text(duration_text(duration)),
        CqlValue::Inet(address) => Value::Text(address.to_string()),
        CqlValue::Uuid(uuid) => Value::Text(uuid.to_string()),
        CqlValue::Timeuuid(uuid) => Value::Text(uuid.to_string()),
        compound => Value::Text(compound.to_string()),
    }
}

fn widen_float(number: f32) -> f64 {
    number.to_string().parse().unwrap_or(f64::from(number))
}

pub(crate) fn timestamp_text(millis: i64) -> String {
    match DateTime::from_timestamp_millis(millis) {
        Some(moment) => moment.format("%Y-%m-%d %H:%M:%S%.3f%z").to_string(),
        None => millis.to_string(),
    }
}

fn date_text(date: CqlDate) -> String {
    let days = i64::from(date.0) - DATE_EPOCH_OFFSET;
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1);
    let shifted = epoch.and_then(|epoch| match u64::try_from(days) {
        Ok(forward) => epoch.checked_add_days(Days::new(forward)),
        Err(_) => epoch.checked_sub_days(Days::new(days.unsigned_abs())),
    });
    match shifted {
        Some(day) => day.format("%Y-%m-%d").to_string(),
        None => days.to_string(),
    }
}

fn time_text(time: CqlTime) -> String {
    let nanos = time.0;
    format!(
        "{:02}:{:02}:{:02}.{:09}",
        nanos / 3_600_000_000_000,
        nanos / 60_000_000_000 % 60,
        nanos / NANOS_PER_SECOND % 60,
        nanos % NANOS_PER_SECOND
    )
}

fn duration_text(duration: CqlDuration) -> String {
    let negative = duration.months < 0 || duration.days < 0 || duration.nanoseconds < 0;
    let parts = [
        (i64::from(duration.months).unsigned_abs(), "mo"),
        (i64::from(duration.days).unsigned_abs(), "d"),
        (duration.nanoseconds.unsigned_abs(), "ns"),
    ];
    let mut text: String = parts
        .iter()
        .filter(|(amount, _)| *amount > 0)
        .map(|(amount, unit)| format!("{amount}{unit}"))
        .collect();
    if text.is_empty() {
        text.push_str("0s");
    }
    if negative {
        text.insert(0, '-');
    }
    text
}

fn varint_text(varint: &CqlVarint) -> String {
    let bytes = varint.as_signed_bytes_be_slice();
    match signed_be_to_i128(bytes) {
        Some(number) => number.to_string(),
        None => CqlValue::Varint(varint.clone()).to_string(),
    }
}

fn decimal_text(decimal: &CqlDecimal) -> String {
    let (bytes, scale) = decimal.as_signed_be_bytes_slice_and_exponent();
    let Some(unscaled) = signed_be_to_i128(bytes) else {
        return CqlValue::Decimal(decimal.clone()).to_string();
    };
    scaled_text(unscaled, scale)
}

fn scaled_text(unscaled: i128, scale: i32) -> String {
    let digits = unscaled.unsigned_abs().to_string();
    let sign = if unscaled < 0 { "-" } else { "" };
    let Ok(scale) = usize::try_from(scale) else {
        let zeros = "0".repeat(scale.unsigned_abs() as usize);
        return format!("{sign}{digits}{zeros}");
    };
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (whole, fraction) = padded.split_at(padded.len() - scale);
    format!("{sign}{whole}.{fraction}")
}

fn signed_be_to_i128(bytes: &[u8]) -> Option<i128> {
    if bytes.is_empty() {
        return Some(0);
    }
    if bytes.len() > 16 {
        return None;
    }
    let fill = if bytes[0] & 0x80 != 0 { 0xff } else { 0x00 };
    let mut buffer = [fill; 16];
    buffer[16 - bytes.len()..].copy_from_slice(bytes);
    Some(i128::from_be_bytes(buffer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use scylla::value::{CqlTimestamp, CqlVarint};

    #[test]
    fn scalars_keep_their_types() {
        assert_eq!(to_value(None), Value::Null);
        assert_eq!(to_value(Some(CqlValue::Int(7))), Value::Int(7));
        assert_eq!(to_value(Some(CqlValue::Float(0.1))), Value::Float(0.1));
        assert_eq!(
            to_value(Some(CqlValue::Text("a".into()))),
            Value::Text("a".into())
        );
        assert_eq!(
            to_value(Some(CqlValue::Blob(vec![1, 2]))),
            Value::Bytes(vec![1, 2])
        );
    }

    #[test]
    fn temporal_values_use_cql_literal_syntax() {
        assert_eq!(
            to_value(Some(CqlValue::Timestamp(CqlTimestamp(1_700_000_000_123)))),
            Value::Text("2023-11-14 22:13:20.123+0000".into())
        );
        assert_eq!(
            to_value(Some(CqlValue::Date(CqlDate((1 << 31) + 1)))),
            Value::Text("1970-01-02".into())
        );
        assert_eq!(
            to_value(Some(CqlValue::Date(CqlDate((1 << 31) - 1)))),
            Value::Text("1969-12-31".into())
        );
        assert_eq!(
            to_value(Some(CqlValue::Time(CqlTime(3_723_000_000_005)))),
            Value::Text("01:02:03.000000005".into())
        );
        let duration = CqlDuration {
            months: 1,
            days: 2,
            nanoseconds: 3,
        };
        assert_eq!(duration_text(duration), "1mo2d3ns");
    }

    #[test]
    fn exact_numbers_are_rendered_as_text() {
        assert_eq!(scaled_text(12345, 2), "123.45");
        assert_eq!(scaled_text(-5, 3), "-0.005");
        assert_eq!(scaled_text(12, -2), "1200");
        let varint = CqlVarint::from_signed_bytes_be(vec![0xff, 0x00]);
        assert_eq!(varint_text(&varint), "-256");
        let huge = CqlVarint::from_signed_bytes_be(vec![1; 20]);
        assert!(varint_text(&huge).starts_with("blobAsVarint(0x"));
    }

    #[test]
    fn collections_become_cql_literals() {
        let list = CqlValue::List(vec![CqlValue::Text("it's".into()), CqlValue::Int(2)]);
        assert_eq!(to_value(Some(list)), Value::Text("['it''s',2]".into()));
    }
}
