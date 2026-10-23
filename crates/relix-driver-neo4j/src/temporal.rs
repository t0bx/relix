/*
 * @author t0bx
 * @since 24.10.2026
 */

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use neo4rs::{BoltDuration, BoltList, BoltString, BoltType, Row};

/// Renders Bolt temporal values in the ISO 8601 forms Neo4j itself prints, so that the
/// text can be fed back into `date()`, `datetime()`, `duration()` and friends.
pub(crate) fn temporal_text(value: &BoltType) -> Option<String> {
    let text = match value {
        BoltType::Date(date) => NaiveDate::try_from(date)
            .ok()?
            .format("%Y-%m-%d")
            .to_string(),
        BoltType::Time(time) => {
            let (time, offset) = <(NaiveTime, FixedOffset)>::from(time);
            format!("{}{offset}", time.format("%H:%M:%S%.f"))
        }
        BoltType::LocalTime(time) => NaiveTime::from(time).format("%H:%M:%S%.f").to_string(),
        BoltType::DateTime(moment) => DateTime::<FixedOffset>::try_from(moment).ok()?.to_rfc3339(),
        BoltType::LocalDateTime(moment) => NaiveDateTime::try_from(moment)
            .ok()?
            .format("%Y-%m-%dT%H:%M:%S%.f")
            .to_string(),
        BoltType::DateTimeZoneId(moment) => {
            let zoned = DateTime::<FixedOffset>::try_from(moment).ok()?;
            format!("{}[{}]", zoned.to_rfc3339(), moment.tz_id())
        }
        BoltType::Duration(duration) => duration_text(duration)?,
        _ => return None,
    };
    Some(text)
}

/// The components of a Bolt duration are private in neo4rs; its public deserializer
/// exposes them as a `(months, days, seconds, nanoseconds)` tuple, read here through a
/// single-value row.
fn duration_parts(duration: &BoltDuration) -> Option<(i64, i64, i64, i64)> {
    let mut fields = BoltList::new();
    fields.push(BoltType::String(BoltString::new("d")));
    let mut data = BoltList::new();
    data.push(BoltType::Duration(duration.clone()));
    Row::new(fields, data).get("d").ok()
}

fn duration_text(duration: &BoltDuration) -> Option<String> {
    let (months, days, seconds, nanoseconds) = duration_parts(duration)?;
    Some(iso_duration(months, days, seconds, nanoseconds))
}

fn iso_duration(months: i64, days: i64, seconds: i64, nanoseconds: i64) -> String {
    let mut text = String::from("P");
    if months != 0 {
        text.push_str(&format!("{months}M"));
    }
    if days != 0 {
        text.push_str(&format!("{days}D"));
    }
    let total_nanos = i128::from(seconds) * 1_000_000_000 + i128::from(nanoseconds);
    if total_nanos != 0 || (months == 0 && days == 0) {
        let sign = if total_nanos < 0 { "-" } else { "" };
        let magnitude = total_nanos.unsigned_abs();
        let whole = magnitude / 1_000_000_000;
        let fraction = magnitude % 1_000_000_000;
        text.push('T');
        text.push_str(&format!("{sign}{whole}"));
        if fraction != 0 {
            let digits = format!("{fraction:09}");
            text.push('.');
            text.push_str(digits.trim_end_matches('0'));
        }
        text.push('S');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_use_iso_8601() {
        assert_eq!(iso_duration(14, 3, 3_661, 500_000_000), "P14M3DT3661.5S");
        assert_eq!(iso_duration(0, 0, 0, 0), "PT0S");
        assert_eq!(iso_duration(0, 2, 0, 0), "P2D");
        assert_eq!(iso_duration(0, 0, -1, -500_000_000), "PT-1.5S");
    }

    #[test]
    fn duration_components_are_read_through_the_public_deserializer() {
        let duration = BoltDuration::new(1.into(), 2.into(), 3.into(), 4.into());
        assert_eq!(duration_parts(&duration), Some((1, 2, 3, 4)));
        assert_eq!(
            temporal_text(&BoltType::Duration(duration)).as_deref(),
            Some("P1M2DT3.000000004S")
        );
    }
}
