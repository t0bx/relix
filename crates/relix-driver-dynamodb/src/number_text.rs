/*
 * @author t0bx
 * @since 23.10.2026
 */

use serde_json::{Number, Value as Json};

/// Converts a DynamoDB number (decimal text with up to 38 significant digits) into a
/// JSON number when that is exact: integers that fit 64 bits, and decimals whose
/// shortest `f64` representation denotes the same decimal value. Anything else returns
/// `None` so the caller keeps the original text.
pub fn exact_json_number(text: &str) -> Option<Json> {
    if let Ok(integer) = text.parse::<i64>() {
        return Some(Json::from(integer));
    }
    if let Ok(integer) = text.parse::<u64>() {
        return Some(Json::from(integer));
    }
    let float: f64 = text.parse().ok().filter(|value: &f64| value.is_finite())?;
    if is_integral_text(text) {
        return None;
    }
    let same_value = canonical_decimal(&float.to_string()) == canonical_decimal(text);
    same_value
        .then(|| Number::from_f64(float))
        .flatten()
        .map(Json::Number)
}

fn is_integral_text(text: &str) -> bool {
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Normalises decimal text to `(negative, significant digits, exponent)` so that
/// `"1.50"`, `"15e-1"` and `"1.5"` compare equal.
fn canonical_decimal(text: &str) -> Option<(bool, String, i64)> {
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i64>().ok()?),
        None => (unsigned, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if !(whole.bytes().chain(fraction.bytes())).all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let digits = format!("{whole}{fraction}");
    let exponent = exponent - fraction.len() as i64;
    let leading_trimmed = digits.trim_start_matches('0');
    if leading_trimmed.is_empty() {
        return Some((false, "0".to_owned(), 0));
    }
    let significant = leading_trimmed.trim_end_matches('0');
    let trailing_zeros = (leading_trimmed.len() - significant.len()) as i64;
    Some((negative, significant.to_owned(), exponent + trailing_zeros))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn integers_and_short_decimals_become_json_numbers() {
        assert_eq!(exact_json_number("42"), Some(json!(42)));
        assert_eq!(exact_json_number("-7"), Some(json!(-7)));
        assert_eq!(
            exact_json_number("18446744073709551615"),
            Some(json!(u64::MAX))
        );
        assert_eq!(exact_json_number("1.5"), Some(json!(1.5)));
        assert_eq!(exact_json_number("0.1"), Some(json!(0.1)));
        assert_eq!(exact_json_number("1.50"), Some(json!(1.5)));
    }

    #[test]
    fn values_beyond_f64_precision_stay_text() {
        assert_eq!(exact_json_number("12345678901234567890123"), None);
        assert_eq!(exact_json_number("0.12345678901234567890123"), None);
        assert_eq!(exact_json_number("not a number"), None);
    }

    #[test]
    fn decimal_text_is_compared_by_value() {
        assert_eq!(canonical_decimal("1.50"), canonical_decimal("15e-1"));
        assert_eq!(canonical_decimal("100"), canonical_decimal("1E+2"));
        assert_eq!(canonical_decimal("0.000"), canonical_decimal("0"));
        assert_ne!(canonical_decimal("-1"), canonical_decimal("1"));
    }
}
