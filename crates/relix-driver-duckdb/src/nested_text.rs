/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::binary_text::{bignum_text, bit_string, blob_text};
use crate::temporal::{format_date, format_interval, format_time, format_timestamp};
use crate::type_names::ColumnType;
use duckdb::core::LogicalTypeId;
use duckdb::types::Value as DuckValue;

const SPECIAL_CHARACTERS: [char; 11] = ['[', ']', '{', '}', '(', ')', ',', ':', '=', '\'', '"'];
const EXPONENT_FROM: i32 = 16;
const EXPONENT_BELOW: i32 = -4;

/// Renders a `LIST`, `ARRAY`, `STRUCT`, `MAP` or `UNION` value exactly like DuckDB casts
/// it to `VARCHAR`: struct keys are always quoted, other strings only when they contain
/// a structural character, surrounding whitespace, are empty or spell `NULL`.
pub(crate) fn render_nested(value: &DuckValue, column: Option<&ColumnType>) -> String {
    let mut out = String::new();
    write_value(&mut out, value, column, false);
    out
}

fn write_value(out: &mut String, value: &DuckValue, column: Option<&ColumnType>, quote: bool) {
    let child = |index: usize| column.and_then(|column| column.child(index));
    match value {
        DuckValue::Null => out.push_str("NULL"),
        DuckValue::List(items) | DuckValue::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                separate(out, index);
                write_value(out, item, child(0), true);
            }
            out.push(']');
        }
        DuckValue::Struct(fields) => {
            out.push('{');
            for (index, (name, field)) in fields.iter().enumerate() {
                separate(out, index);
                out.push_str(&quoted(name));
                out.push_str(": ");
                write_value(out, field, child(index), true);
            }
            out.push('}');
        }
        DuckValue::Map(entries) => {
            out.push('{');
            for (index, (key, entry)) in entries.iter().enumerate() {
                separate(out, index);
                write_value(out, key, child(0), true);
                out.push('=');
                write_value(out, entry, child(1), true);
            }
            out.push('}');
        }
        DuckValue::Union(member) => write_value(out, member, None, quote),
        leaf => {
            let text = leaf_text(leaf, column);
            if quote && needs_quotes(&text) {
                out.push_str(&quoted(&text));
            } else {
                out.push_str(&text);
            }
        }
    }
}

fn separate(out: &mut String, index: usize) {
    if index > 0 {
        out.push_str(", ");
    }
}

fn leaf_text(value: &DuckValue, column: Option<&ColumnType>) -> String {
    let id = column.map(|column| column.id);
    match value {
        DuckValue::Boolean(flag) => flag.to_string(),
        DuckValue::TinyInt(number) => number.to_string(),
        DuckValue::SmallInt(number) => number.to_string(),
        DuckValue::Int(number) => number.to_string(),
        DuckValue::BigInt(number) => number.to_string(),
        DuckValue::HugeInt(number) if id == Some(LogicalTypeId::UHugeint) => {
            number.cast_unsigned().to_string()
        }
        DuckValue::HugeInt(number) => number.to_string(),
        DuckValue::UHugeInt(number) => number.to_string(),
        DuckValue::UTinyInt(number) => number.to_string(),
        DuckValue::USmallInt(number) => number.to_string(),
        DuckValue::UInt(number) => number.to_string(),
        DuckValue::UBigInt(number) => number.to_string(),
        DuckValue::Float(number) => real_text(
            number.is_nan(),
            number.is_infinite(),
            *number < 0.0,
            format!("{number:e}"),
            number.to_string(),
        ),
        DuckValue::Double(number) => real_text(
            number.is_nan(),
            number.is_infinite(),
            *number < 0.0,
            format!("{number:e}"),
            number.to_string(),
        ),
        DuckValue::Decimal(decimal) => decimal.to_string(),
        DuckValue::Timestamp(unit, ticks) => {
            format_timestamp(*unit, *ticks, column.is_some_and(ColumnType::has_time_zone))
        }
        DuckValue::Date32(days) => format_date(*days),
        DuckValue::Time64(unit, ticks) => format_time(*unit, *ticks),
        DuckValue::Interval {
            months,
            days,
            nanos,
        } => format_interval(*months, *days, *nanos),
        DuckValue::Text(text) | DuckValue::Enum(text) => text.clone(),
        DuckValue::Blob(bytes) => match id {
            Some(LogicalTypeId::Bit) => bit_string(bytes),
            Some(LogicalTypeId::Bignum) => bignum_text(bytes).unwrap_or_else(|| blob_text(bytes)),
            _ => blob_text(bytes),
        },
        DuckValue::Geometry(bytes) => blob_text(bytes),
        other => format!("{other:?}"),
    }
}

/// Formats `FLOAT`/`DOUBLE` like DuckDB: the shortest round-trip digits, in fixed
/// notation with at least one decimal for exponents in `[-4, 16)`, otherwise in
/// scientific notation with a signed, two digit exponent (`1e+16`, `2.5e-07`).
fn real_text(
    nan: bool,
    infinite: bool,
    negative: bool,
    scientific: String,
    fixed: String,
) -> String {
    if nan {
        return "nan".to_owned();
    }
    if infinite {
        return if negative { "-inf" } else { "inf" }.to_owned();
    }
    let (mantissa, exponent) = scientific
        .split_once('e')
        .map_or((scientific.as_str(), 0), |(mantissa, exponent)| {
            (mantissa, exponent.parse().unwrap_or(0))
        });
    if !(EXPONENT_BELOW..EXPONENT_FROM).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}e{sign}{:02}", exponent.unsigned_abs());
    }
    if fixed.contains('.') {
        fixed
    } else {
        format!("{fixed}.0")
    }
}

fn needs_quotes(text: &str) -> bool {
    text.is_empty()
        || text.eq_ignore_ascii_case("null")
        || text.starts_with(char::is_whitespace)
        || text.ends_with(char::is_whitespace)
        || text.contains(SPECIAL_CHARACTERS)
}

fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for ch in text.chars() {
        if matches!(ch, '\'' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use crate::rows::{execute_streaming, read_batch};

    fn assert_matches_duckdb_cast(expression: &str) {
        let connection = duckdb::Connection::open_in_memory().unwrap();
        let sql = format!("SELECT {expression}, ({expression})::VARCHAR");
        let mut statement = connection.prepare(&sql).unwrap();
        let columns = execute_streaming(&mut statement, [], &sql).unwrap();
        let batch = read_batch(&statement, &columns, 1).unwrap();
        assert_eq!(batch.rows[0][0], batch.rows[0][1], "{expression}");
    }

    #[test]
    fn strings_are_quoted_only_when_ambiguous() {
        assert_matches_duckdb_cast(
            r"['a', 'b c', NULL, 'it''s \ x', '', 'a	b', '	lead', 'x(y)', 'a#b', 'null', 'a\b']",
        );
        assert_matches_duckdb_cast(
            "['a,b', '[x]', '{y}', 'a:b', 'a=b', ' lead', 'trail ', 'quote\"d', 'new\nline']",
        );
    }

    #[test]
    fn structs_and_maps_follow_duckdb_syntax() {
        assert_matches_duckdb_cast(r"{'it''s': 1, 'a\b': [1, 2], 'x y': {'c': 'p''q'}}");
        assert_matches_duckdb_cast("MAP {'a=b': 'c=d', 'k': NULL}");
        assert_matches_duckdb_cast("MAP {[1]: {'x': 1}}");
        assert_matches_duckdb_cast("[{'a': NULL}, NULL]");
        assert_matches_duckdb_cast("[[], NULL, [NULL]]");
        assert_matches_duckdb_cast("[union_value(s := 'a b')]");
        assert_matches_duckdb_cast("[[1,2]::INTEGER[2]]");
    }

    #[test]
    fn numbers_render_like_duckdb() {
        assert_matches_duckdb_cast("[1.5::FLOAT, 0.1::FLOAT, 1e20::FLOAT, -3.25e-9::FLOAT]");
        assert_matches_duckdb_cast(
            "[1e15::DOUBLE, 1e16::DOUBLE, 0.0001::DOUBLE, 0.00001::DOUBLE, 1.0::DOUBLE, \
             -2.5e-7::DOUBLE, 123456789.125::DOUBLE, 'nan'::DOUBLE, 'inf'::DOUBLE, \
             '-inf'::DOUBLE, -0.0::DOUBLE]",
        );
        assert_matches_duckdb_cast("[340282366920938463463374607431768211455::UHUGEINT]");
        assert_matches_duckdb_cast("[-170141183460469231731687303715884105727::HUGEINT]");
        assert_matches_duckdb_cast("[1.5::DECIMAL(38,10)]");
        assert_matches_duckdb_cast("[2::DECIMAL(38,0), NULL]");
        assert_matches_duckdb_cast("[123::BIGNUM, -5::BIGNUM]");
    }

    #[test]
    fn temporal_and_binary_values_render_like_duckdb() {
        assert_matches_duckdb_cast(
            "[TIMESTAMP '2024-01-02 03:04:05.5', TIMESTAMP '1969-12-31 23:59:59.999999']",
        );
        assert_matches_duckdb_cast("[TIMESTAMPTZ '2024-01-02 03:04:05.5']");
        assert_matches_duckdb_cast("[TIMESTAMP_NS '2024-01-02 03:04:05.123456789']");
        assert_matches_duckdb_cast("[TIMESTAMP_S '2024-01-02 03:04:05']");
        assert_matches_duckdb_cast("[DATE '2024-02-29', '0001-01-01'::DATE - 1, 'infinity'::DATE]");
        assert_matches_duckdb_cast("[TIME '12:00:00.5', TIME '00:00:00']");
        assert_matches_duckdb_cast(
            "[INTERVAL '1 year 2 months 3 days 04:05:06.7', INTERVAL '-1 day', \
             INTERVAL '0 seconds', INTERVAL '-1 hour -30 minutes', INTERVAL '25 hours']",
        );
        assert_matches_duckdb_cast(r"['\xAA\xbb a\x00'::BLOB, 'it''s\x5C'::BLOB]");
        assert_matches_duckdb_cast("['101'::BIT, '1'::BIT]");
        assert_matches_duckdb_cast("[true, false]");
        assert_matches_duckdb_cast("['a'::ENUM('a', 'b c')]");
    }
}
