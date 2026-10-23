/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Value;

pub(crate) fn text_at(row: &[Value], index: usize) -> String {
    row.get(index)
        .and_then(Value::to_plain_string)
        .unwrap_or_default()
}

pub(crate) fn optional_text_at(row: &[Value], index: usize) -> Option<String> {
    Some(text_at(row, index)).filter(|text| !text.is_empty())
}

pub(crate) fn flag_at(row: &[Value], index: usize) -> bool {
    match row.get(index) {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Int(number)) => *number != 0,
        Some(Value::UInt(number)) => *number != 0,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_are_read_leniently() {
        let row = vec![Value::Text("x".into()), Value::Int(1), Value::Null];
        assert_eq!(text_at(&row, 0), "x");
        assert!(flag_at(&row, 1));
        assert_eq!(optional_text_at(&row, 2), None);
        assert_eq!(text_at(&row, 9), "");
    }
}
