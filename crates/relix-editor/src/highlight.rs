/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Dialect;
use relix_query::lexer::{TokenKind, tokenize};
use relix_query::vocabulary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightClass {
    Plain,
    Keyword,
    Identifier,
    Function,
    DataType,
    String,
    Number,
    Comment,
    Operator,
    Parameter,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightSpan {
    pub start_column: usize,
    pub end_column: usize,
    pub class: HighlightClass,
}

/// Produces highlight spans per line (char columns). The whole text is tokenized at once
/// so multi-line comments and strings are coloured correctly across line breaks.
pub fn highlight_lines(text: &str, dialect: Dialect) -> Vec<Vec<HighlightSpan>> {
    let tokens = tokenize(text, dialect);
    let mut lines: Vec<Vec<HighlightSpan>> = vec![Vec::new()];
    let mut column = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        let class = classify_token(token.kind, token.text, dialect, || {
            tokens[index + 1..]
                .iter()
                .find(|next| next.kind != TokenKind::Whitespace)
                .is_some_and(|next| next.text == "(")
        });
        for (segment_index, segment) in token.text.split('\n').enumerate() {
            if segment_index > 0 {
                lines.push(Vec::new());
                column = 0;
            }
            let width = segment.chars().count();
            if width > 0
                && class != HighlightClass::Plain
                && let Some(line) = lines.last_mut()
            {
                line.push(HighlightSpan {
                    start_column: column,
                    end_column: column + width,
                    class,
                });
            }
            column += width;
        }
    }
    lines
}

fn classify_token(
    kind: TokenKind,
    text: &str,
    dialect: Dialect,
    followed_by_paren: impl FnOnce() -> bool,
) -> HighlightClass {
    match kind {
        TokenKind::Whitespace | TokenKind::Unknown => HighlightClass::Plain,
        TokenKind::LineComment | TokenKind::BlockComment => HighlightClass::Comment,
        TokenKind::String => HighlightClass::String,
        TokenKind::Number => HighlightClass::Number,
        TokenKind::Parameter => HighlightClass::Parameter,
        TokenKind::Operator | TokenKind::Punctuation => HighlightClass::Operator,
        TokenKind::Unterminated => HighlightClass::Error,
        TokenKind::QuotedIdentifier => HighlightClass::Identifier,
        TokenKind::Keyword => HighlightClass::Keyword,
        TokenKind::Identifier => {
            if followed_by_paren() {
                HighlightClass::Function
            } else if vocabulary::data_types(dialect).any(|name| name.eq_ignore_ascii_case(text)) {
                HighlightClass::DataType
            } else {
                HighlightClass::Identifier
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_follow_lines_and_columns() {
        let lines = highlight_lines("SELECT count(*)\n/* a\nb */ 'x'", Dialect::Postgres);
        assert_eq!(lines.len(), 3);
        assert_eq!(
            lines[0][0],
            HighlightSpan {
                start_column: 0,
                end_column: 6,
                class: HighlightClass::Keyword
            }
        );
        assert_eq!(lines[0][1].class, HighlightClass::Function);
        assert_eq!(lines[1][0].class, HighlightClass::Comment);
        assert_eq!(
            lines[2][0],
            HighlightSpan {
                start_column: 0,
                end_column: 4,
                class: HighlightClass::Comment
            }
        );
        assert_eq!(lines[2][1].class, HighlightClass::String);
    }

    #[test]
    fn data_types_and_errors() {
        let lines = highlight_lines("x::jsonb 'open", Dialect::Postgres);
        assert!(
            lines[0]
                .iter()
                .any(|span| span.class == HighlightClass::DataType)
        );
        assert_eq!(lines[0].last().unwrap().class, HighlightClass::Error);
    }
}
