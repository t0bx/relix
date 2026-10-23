/*
 * @author t0bx
 * @since 23.10.2026
 */

use chrono::{Duration, NaiveDate};
use tiberius::time::{Date, DateTime, DateTime2, DateTimeOffset, SmallDateTime, Time};

const NANOS_PER_SECOND: u64 = 1_000_000_000;
const SECONDS_PER_DAY: u64 = 86_400;
const LEGACY_FRAGMENTS_PER_SECOND: u64 = 300;
const MAX_FRACTION_DIGITS: usize = 7;

pub fn format_date(date: Date) -> String {
    days_since(1, i64::from(date.days())).map_or_else(
        || format!("<invalid date: {} days>", date.days()),
        |date| date.format("%Y-%m-%d").to_string(),
    )
}

pub fn format_time(time: Time) -> String {
    clock_text(time_nanos(time), MAX_FRACTION_DIGITS)
}

pub fn format_datetime2(value: DateTime2) -> String {
    format!(
        "{} {}",
        format_date(value.date()),
        format_time(value.time())
    )
}

/// Formats the local time of a `datetimeoffset` (the wire value is UTC) with its offset.
pub fn format_datetime_offset(value: DateTimeOffset) -> String {
    let utc = value.datetime2();
    let offset_minutes = i64::from(value.offset());
    let local = days_since(1, i64::from(utc.date().days())).map(|date| {
        let nanos =
            i128::from(time_nanos(utc.time())) + i128::from(offset_minutes) * 60_000_000_000;
        let day_nanos = i128::from(SECONDS_PER_DAY * NANOS_PER_SECOND);
        let day_shift = nanos.div_euclid(day_nanos);
        let clock = u64::try_from(nanos.rem_euclid(day_nanos)).unwrap_or_default();
        (
            date + Duration::days(i64::try_from(day_shift).unwrap_or_default()),
            clock,
        )
    });
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let offset = format!(
        "{sign}{:02}:{:02}",
        offset_minutes.abs() / 60,
        offset_minutes.abs() % 60
    );
    match local {
        Some((date, clock)) => format!(
            "{} {} {offset}",
            date.format("%Y-%m-%d"),
            clock_text(clock, MAX_FRACTION_DIGITS)
        ),
        None => format!("<invalid datetimeoffset> {offset}"),
    }
}

/// Legacy `datetime` stores 1/300 seconds; it is shown with millisecond precision, which
/// is also the precision SQL Server accepts when the text is converted back.
pub fn format_datetime(value: DateTime) -> String {
    let fragments = u64::from(value.seconds_fragments());
    let millis = (fragments * 1000 + LEGACY_FRAGMENTS_PER_SECOND / 2) / LEGACY_FRAGMENTS_PER_SECOND;
    let date = days_since(1900, i64::from(value.days()));
    match date {
        Some(date) => format!(
            "{} {}",
            date.format("%Y-%m-%d"),
            clock_text(millis * 1_000_000, 3)
        ),
        None => format!("<invalid datetime: {} days>", value.days()),
    }
}

pub fn format_small_datetime(value: SmallDateTime) -> String {
    let date = days_since(1900, i64::from(value.days()));
    let minutes = u64::from(value.seconds_fragments());
    match date {
        Some(date) => format!(
            "{} {:02}:{:02}:00",
            date.format("%Y-%m-%d"),
            minutes / 60,
            minutes % 60
        ),
        None => format!("<invalid smalldatetime: {} days>", value.days()),
    }
}

fn days_since(year: i32, days: i64) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(year, 1, 1)?.checked_add_signed(Duration::try_days(days)?)
}

fn time_nanos(time: Time) -> u64 {
    let scale = u32::from(time.scale().min(9));
    time.increments() * 10u64.pow(9 - scale)
}

fn clock_text(nanos: u64, max_fraction_digits: usize) -> String {
    let seconds = nanos / NANOS_PER_SECOND;
    let mut text = format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    let fraction = format!("{:09}", nanos % NANOS_PER_SECOND);
    let fraction = fraction[..max_fraction_digits.min(9)].trim_end_matches('0');
    if !fraction.is_empty() {
        text.push('.');
        text.push_str(fraction);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_count_days_from_year_one() {
        assert_eq!(format_date(Date::new(0)), "0001-01-01");
        assert_eq!(format_date(Date::new(739_546)), "2025-10-23");
    }

    #[test]
    fn times_keep_up_to_seven_fraction_digits() {
        assert_eq!(
            format_time(Time::new(452_961_234_567, 7)),
            "12:34:56.1234567"
        );
        assert_eq!(format_time(Time::new(45_296_000, 3)), "12:34:56");
        assert_eq!(format_time(Time::new(452_965, 1)), "12:34:56.5");
    }

    #[test]
    fn legacy_datetime_rounds_to_milliseconds() {
        assert_eq!(
            format_datetime(DateTime::new(0, 299)),
            "1900-01-01 00:00:00.997"
        );
        assert_eq!(
            format_datetime(DateTime::new(-1, 300 * 61)),
            "1899-12-31 00:01:01"
        );
        assert_eq!(
            format_small_datetime(SmallDateTime::new(1, 61)),
            "1900-01-02 01:01:00"
        );
    }

    #[test]
    fn offsets_shift_the_utc_wire_value() {
        let utc = DateTime2::new(Date::new(739_546), Time::new(23 * 3600 * 10, 1));
        assert_eq!(
            format_datetime_offset(DateTimeOffset::new(utc, 120)),
            "2025-10-24 01:00:00 +02:00"
        );
        assert_eq!(
            format_datetime_offset(DateTimeOffset::new(utc, -90)),
            "2025-10-23 21:30:00 -01:30"
        );
    }
}
