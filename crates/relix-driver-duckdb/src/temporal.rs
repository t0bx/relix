/*
 * @author t0bx
 * @since 23.10.2026
 */

use duckdb::types::TimeUnit;

const DATE_INFINITY: i32 = i32::MAX;
const TIMESTAMP_INFINITY: i64 = i64::MAX;
const SECONDS_PER_DAY: i64 = 86_400;
const MICROS_PER_SECOND: i64 = 1_000_000;

pub(crate) fn format_date(days: i32) -> String {
    match days {
        DATE_INFINITY => "infinity".to_owned(),
        days if days == -DATE_INFINITY => "-infinity".to_owned(),
        days => {
            let (date, era) = civil_date(i64::from(days));
            format!("{date}{era}")
        }
    }
}

pub(crate) fn format_time(unit: TimeUnit, value: i64) -> String {
    let (seconds, fraction) = split_fraction(unit, value);
    clock(seconds, &fraction)
}

pub(crate) fn format_timestamp(unit: TimeUnit, value: i64, with_time_zone: bool) -> String {
    if value == TIMESTAMP_INFINITY {
        return "infinity".to_owned();
    }
    if value == -TIMESTAMP_INFINITY {
        return "-infinity".to_owned();
    }
    let (seconds, fraction) = split_fraction(unit, value);
    let (date, era) = civil_date(seconds.div_euclid(SECONDS_PER_DAY));
    let time = clock(seconds.rem_euclid(SECONDS_PER_DAY), &fraction);
    let zone = if with_time_zone { "+00" } else { "" };
    format!("{date} {time}{zone}{era}")
}

/// Renders an interval the way DuckDB casts it to text: calendar parts first
/// (`1 year 2 months 3 days`), then the clock part, or `00:00:00` when empty.
pub(crate) fn format_interval(months: i32, days: i32, nanos: i64) -> String {
    let mut parts = Vec::new();
    let years = months / 12;
    let remaining_months = months % 12;
    push_calendar_part(&mut parts, i64::from(years), "year");
    push_calendar_part(&mut parts, i64::from(remaining_months), "month");
    push_calendar_part(&mut parts, i64::from(days), "day");
    let micros = nanos / 1_000;
    if micros != 0 || parts.is_empty() {
        let sign = if micros < 0 { "-" } else { "" };
        let magnitude = micros.unsigned_abs();
        let seconds = magnitude / 1_000_000;
        let fraction = trimmed_fraction(magnitude % 1_000_000, 6);
        let hours = seconds / 3_600;
        parts.push(format!(
            "{sign}{hours:02}:{:02}:{:02}{fraction}",
            seconds / 60 % 60,
            seconds % 60
        ));
    }
    parts.join(" ")
}

fn push_calendar_part(parts: &mut Vec<String>, amount: i64, unit: &str) {
    if amount == 0 {
        return;
    }
    let plural = if amount.abs() == 1 { "" } else { "s" };
    parts.push(format!("{amount} {unit}{plural}"));
}

fn split_fraction(unit: TimeUnit, value: i64) -> (i64, String) {
    let (per_second, digits) = match unit {
        TimeUnit::Second => (1, 0),
        TimeUnit::Millisecond => (1_000, 3),
        TimeUnit::Microsecond => (MICROS_PER_SECOND, 6),
        TimeUnit::Nanosecond => (1_000_000_000, 9),
    };
    let seconds = value.div_euclid(per_second);
    let remainder = value.rem_euclid(per_second).unsigned_abs();
    (seconds, trimmed_fraction(remainder, digits))
}

fn trimmed_fraction(value: u64, digits: usize) -> String {
    if value == 0 || digits == 0 {
        return String::new();
    }
    let padded = format!("{value:0digits$}");
    format!(".{}", padded.trim_end_matches('0'))
}

fn clock(seconds_of_day: i64, fraction: &str) -> String {
    format!(
        "{:02}:{:02}:{:02}{fraction}",
        seconds_of_day / 3_600,
        seconds_of_day / 60 % 60,
        seconds_of_day % 60
    )
}

/// Converts days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `civil_from_days`). Years before 1 are numbered like DuckDB does, with a ` (BC)`
/// era suffix that callers append after the time part.
fn civil_date(days: i64) -> (String, &'static str) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    if year <= 0 {
        (format!("{:04}-{month:02}-{day:02}", 1 - year), " (BC)")
    } else {
        (format!("{year:04}-{month:02}-{day:02}"), "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_cover_epoch_leap_days_and_eras() {
        assert_eq!(format_date(0), "1970-01-01");
        assert_eq!(format_date(19_782), "2024-02-29");
        assert_eq!(format_date(-719_162), "0001-01-01");
        assert_eq!(format_date(-719_163), "0001-12-31 (BC)");
        assert_eq!(format_date(i32::MAX), "infinity");
    }

    #[test]
    fn timestamps_trim_fractional_zeros() {
        let micros = 1_704_067_200_000_000 + 45_296_120_000;
        assert_eq!(
            format_timestamp(TimeUnit::Microsecond, micros, false),
            "2024-01-01 12:34:56.12"
        );
        assert_eq!(
            format_timestamp(TimeUnit::Second, -1, true),
            "1969-12-31 23:59:59+00"
        );
        assert_eq!(
            format_timestamp(TimeUnit::Nanosecond, 1, false),
            "1970-01-01 00:00:00.000000001"
        );
    }

    #[test]
    fn times_and_intervals_follow_duckdb_text() {
        assert_eq!(
            format_time(TimeUnit::Microsecond, 3_723_500_000),
            "01:02:03.5"
        );
        assert_eq!(
            format_interval(14, 3, 3_600_000_000_000),
            "1 year 2 months 3 days 01:00:00"
        );
        assert_eq!(format_interval(0, 0, 0), "00:00:00");
        assert_eq!(format_interval(0, -1, -1_500_000), "-1 day -00:00:00.0015");
    }
}
