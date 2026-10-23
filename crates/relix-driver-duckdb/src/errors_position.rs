/*
 * @author t0bx
 * @since 23.10.2026
 */

const ELLIPSIS: &str = "...";

/// Recovers the 0-based character position of an error from DuckDB's source context:
///
/// ```text
/// LINE 3: ...where a = 1 FRM t
///                        ^
/// ```
///
/// The quoted line can be shortened with `...` on either side, so the visible excerpt
/// is located inside the original line before the caret column is translated. DuckDB
/// misplaces the caret after multi-byte characters, so no position is reported when
/// the excerpt before the caret is not plain ASCII.
pub(crate) fn error_position(message: &str, sql: &str) -> Option<usize> {
    let mut lines = message.lines();
    let context = lines.find(|line| line.starts_with("LINE "))?;
    let caret_line = lines.next()?;
    let (label, excerpt) = context.split_once(": ")?;
    let line_number: usize = label.strip_prefix("LINE ")?.trim().parse().ok()?;
    let caret_column = caret_line.chars().position(|ch| ch == '^')?;
    let column_in_excerpt = caret_column.checked_sub(label.len() + 2)?;
    let source_line = sql.lines().nth(line_number.checked_sub(1)?)?;
    let (visible, skipped) = match excerpt.strip_prefix(ELLIPSIS) {
        Some(rest) => (rest, ELLIPSIS.len()),
        None => (excerpt, 0),
    };
    let visible = visible.strip_suffix(ELLIPSIS).unwrap_or(visible);
    let byte_start = source_line.find(visible)?;
    let column_in_visible = column_in_excerpt.checked_sub(skipped)?;
    if !visible.get(..column_in_visible)?.is_ascii() {
        return None;
    }
    let column = source_line[..byte_start].chars().count() + column_in_visible;
    let line_start: usize = sql
        .split_inclusive('\n')
        .take(line_number - 1)
        .map(|line| line.chars().count())
        .sum();
    Some(line_start + column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_column_becomes_statement_position() {
        let message = "Parser Error: syntax error at or near \"FRM\"\n\n\
                       LINE 1: SELECT 1 FRM t\n                 ^";
        assert_eq!(error_position(message, "SELECT 1 FRM t"), Some(9));
    }

    #[test]
    fn later_lines_and_shortened_excerpts_are_resolved() {
        let sql = "SELECT 'ä'\nFROM t WHERE aaaaaaaaaa = 1 FRM";
        let message = "Parser Error: x\n\nLINE 2: ...aaaaaa = 1 FRM\n                      ^";
        assert_eq!(error_position(message, sql), Some(11 + 28));
    }

    #[test]
    fn carets_after_multi_byte_characters_are_ignored() {
        let message = "Parser Error: syntax error at or near \"t\"\n\n\
                       LINE 1: SELECT 'ä' FRM t\n                      ^";
        assert_eq!(error_position(message, "SELECT 'ä' FRM t"), None);
    }

    #[test]
    fn messages_without_context_have_no_position() {
        assert_eq!(error_position("Catalog Error: missing", "SELECT 1"), None);
    }
}
