/*
 * @author t0bx
 * @since 23.10.2026
 */

/// Converts a 1-based line and 0-based column, as reported in server error messages,
/// into a 0-based character offset within `text`.
pub fn char_offset(text: &str, line: usize, column: usize) -> Option<usize> {
    let line_index = line.checked_sub(1)?;
    let mut offset = 0;
    for (index, content) in text.split('\n').enumerate() {
        let length = content.chars().count();
        if index == line_index {
            return (column <= length).then_some(offset + column);
        }
        offset += length + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_count_characters_across_lines() {
        let text = "SELECT 1\nFROMM ä\nWHERE";
        assert_eq!(char_offset(text, 1, 7), Some(7));
        assert_eq!(char_offset(text, 2, 6), Some(15));
        assert_eq!(char_offset(text, 3, 0), Some(17));
        assert_eq!(char_offset(text, 4, 0), None);
        assert_eq!(char_offset(text, 0, 0), None);
        assert_eq!(char_offset(text, 1, 40), None);
    }
}
