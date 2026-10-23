/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{SortDirection, Value};
use std::cmp::Ordering;

/// Total order used for client-side sorting: NULLs sort last, numbers compare
/// numerically across integer/float variants, numeric-looking text compares as numbers
/// and everything else falls back to case-sensitive text comparison.
pub fn compare_values(left: &Value, right: &Value) -> Ordering {
    match (left, right) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Greater,
        (_, Value::Null) => Ordering::Less,
        _ => match (as_number(left), as_number(right)) {
            (Some(a), Some(b)) => a.total_cmp(&b),
            _ => text_of(left).cmp(&text_of(right)),
        },
    }
}

pub fn sort_rows(rows: &mut [Vec<Value>], keys: &[(usize, SortDirection)]) {
    rows.sort_by(|left, right| {
        for (column, direction) in keys {
            let ordering = match (left.get(*column), right.get(*column)) {
                (Some(a), Some(b)) => compare_values(a, b),
                _ => Ordering::Equal,
            };
            let ordering = match direction {
                SortDirection::Ascending => ordering,
                SortDirection::Descending => ordering.reverse(),
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    });
}

fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Int(number) => Some(*number as f64),
        Value::UInt(number) => Some(*number as f64),
        Value::Float(number) => Some(*number),
        Value::Text(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite()),
        _ => None,
    }
}

fn text_of(value: &Value) -> String {
    value.to_plain_string().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_by_multiple_keys_with_nulls_last() {
        let mut rows = vec![
            vec![Value::Text("b".into()), Value::Int(2)],
            vec![Value::Null, Value::Int(1)],
            vec![Value::Text("a".into()), Value::Int(9)],
            vec![Value::Text("b".into()), Value::Int(1)],
        ];
        sort_rows(
            &mut rows,
            &[
                (0, SortDirection::Ascending),
                (1, SortDirection::Descending),
            ],
        );
        let firsts: Vec<_> = rows.iter().map(|row| row[1].clone()).collect();
        assert_eq!(
            firsts,
            vec![Value::Int(9), Value::Int(2), Value::Int(1), Value::Int(1)]
        );
        assert_eq!(rows[3][0], Value::Null);
    }

    #[test]
    fn numeric_text_compares_numerically() {
        assert_eq!(
            compare_values(&Value::Text("10".into()), &Value::Text("9".into())),
            Ordering::Greater
        );
        assert_eq!(
            compare_values(&Value::Int(3), &Value::Float(2.5)),
            Ordering::Greater
        );
    }
}
