/*
 * @author t0bx
 * @since 23.10.2026
 */

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime, TimeDelta};

const NANOS_PER_SECOND: i128 = 1_000_000_000;
const NANOSECOND_EPOCH_THRESHOLD: i128 = 100_000_000_000_000;
const TIME_ZONE_BIAS_MINUTES: i32 = 1440;
const DATE_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimestampZone {
    Naive,
    Utc,
    Encoded,
}

/// `DATE` values arrive as the number of days since 1970-01-01.
pub(crate) fn date_from_days(raw: &str) -> Option<String> {
    let days: i64 = raw.trim().parse().ok()?;
    let date =
        NaiveDate::from_ymd_opt(1970, 1, 1)?.checked_add_signed(TimeDelta::try_days(days)?)?;
    Some(date.format("%Y-%m-%d").to_string())
}

/// `TIME` values arrive as seconds since midnight with nine fractional digits.
pub(crate) fn time_of_day(raw: &str, scale: u32) -> Option<String> {
    let (seconds, nanos) = epoch_parts(raw.trim())?;
    let time = NaiveTime::from_num_seconds_from_midnight_opt(u32::try_from(seconds).ok()?, nanos)?;
    Some(format!(
        "{}{}",
        time.format("%H:%M:%S"),
        fraction(nanos, scale)
    ))
}

/// Timestamps arrive as seconds since the epoch with nine fractional digits;
/// `TIMESTAMP_TZ` appends the offset as minutes biased by 1440 (`… 960` is UTC-08:00).
pub(crate) fn timestamp(raw: &str, scale: u32, zone: TimestampZone) -> Option<String> {
    let mut parts = raw.split_whitespace();
    let (seconds, nanos) = epoch_parts(parts.next()?)?;
    let instant = DateTime::from_timestamp(seconds, nanos)?;
    let fraction = fraction(nanos, scale);
    match zone {
        TimestampZone::Naive => Some(format!(
            "{}{fraction}",
            instant.naive_utc().format(DATE_TIME_FORMAT)
        )),
        TimestampZone::Utc => Some(format!(
            "{}{fraction} +00:00",
            instant.format(DATE_TIME_FORMAT)
        )),
        TimestampZone::Encoded => {
            let biased: i32 = parts.next()?.parse().ok()?;
            let offset = FixedOffset::east_opt((biased - TIME_ZONE_BIAS_MINUTES) * 60)?;
            let local = instant.with_timezone(&offset);
            Some(format!(
                "{}{fraction} {}",
                local.format(DATE_TIME_FORMAT),
                local.format("%:z")
            ))
        }
    }
}

/// Splits `seconds[.fraction]` into whole seconds (rounded towards negative infinity)
/// and nanoseconds. Integers too large to be seconds are read as nanoseconds.
fn epoch_parts(raw: &str) -> Option<(i64, u32)> {
    let (whole, fraction) = raw.split_once('.').unwrap_or((raw, ""));
    let negative = whole.starts_with('-');
    let whole: i128 = whole.parse().ok()?;
    if fraction.is_empty() && whole.abs() >= NANOSECOND_EPOCH_THRESHOLD {
        return split_nanos(whole);
    }
    let digits: String = fraction.chars().take(9).collect();
    if !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let nanos: i128 = format!("{digits:0<9}").parse().ok()?;
    let signed_nanos = if negative { -nanos } else { nanos };
    split_nanos(whole * NANOS_PER_SECOND + signed_nanos)
}

fn split_nanos(total: i128) -> Option<(i64, u32)> {
    let seconds = i64::try_from(total.div_euclid(NANOS_PER_SECOND)).ok()?;
    let nanos = u32::try_from(total.rem_euclid(NANOS_PER_SECOND)).ok()?;
    Some((seconds, nanos))
}

fn fraction(nanos: u32, scale: u32) -> String {
    let digits = scale.min(9) as usize;
    if digits == 0 {
        return String::new();
    }
    let all = format!("{nanos:09}");
    format!(".{}", &all[..digits])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_count_days_from_the_epoch() {
        assert_eq!(date_from_days("18262").as_deref(), Some("2020-01-01"));
        assert_eq!(date_from_days("-1").as_deref(), Some("1969-12-31"));
        assert_eq!(date_from_days("x"), None);
    }

    #[test]
    fn times_use_the_column_scale() {
        assert_eq!(
            time_of_day("82919.123456789", 3).as_deref(),
            Some("23:01:59.123")
        );
        assert_eq!(time_of_day("0.000000000", 0).as_deref(), Some("00:00:00"));
    }

    #[test]
    fn timestamps_render_per_zone_kind() {
        assert_eq!(
            timestamp("1616173619.500000000", 3, TimestampZone::Naive).as_deref(),
            Some("2021-03-19 17:06:59.500")
        );
        assert_eq!(
            timestamp("1616173619.000000000", 0, TimestampZone::Utc).as_deref(),
            Some("2021-03-19 17:06:59 +00:00")
        );
        assert_eq!(
            timestamp("1616173619.000000000 960", 0, TimestampZone::Encoded).as_deref(),
            Some("2021-03-19 09:06:59 -08:00")
        );
        assert_eq!(
            timestamp("1616173619000000000 1440", 0, TimestampZone::Encoded).as_deref(),
            Some("2021-03-19 17:06:59 +00:00")
        );
    }

    #[test]
    fn negative_fractions_round_down() {
        assert_eq!(
            timestamp("-0.500000000", 1, TimestampZone::Naive).as_deref(),
            Some("1969-12-31 23:59:59.5")
        );
    }
}
