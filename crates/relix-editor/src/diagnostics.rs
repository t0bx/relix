/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::buffer::TextBuffer;
use relix_core::Dialect;
use relix_query::lexer::{TokenKind, tokenize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize,
    pub start_column: usize,
    pub end_column: usize,
    pub message: String,
    pub severity: Severity,
}

impl Diagnostic {
    /// Places a server error inside the buffer. `statement_offset` is the byte offset of
    /// the failing statement in the buffer and `char_position` the 0-based character
    /// offset the database reported within that statement. The marker spans the word
    /// starting at that position.
    pub fn from_server_error(
        buffer: &TextBuffer,
        statement_offset: usize,
        char_position: Option<usize>,
        message: &str,
    ) -> Self {
        let text = buffer.text();
        let statement = text.get(statement_offset..).unwrap_or_default();
        let byte_in_statement = char_position
            .and_then(|position| statement.char_indices().nth(position))
            .map_or(0, |(byte, _)| byte);
        let start = buffer.position_of(statement_offset + byte_in_statement);
        let word_length = buffer
            .line(start.line)
            .chars()
            .skip(start.column)
            .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
            .count()
            .max(1);
        Self {
            line: start.line,
            start_column: start.column,
            end_column: start.column + word_length,
            message: message.to_owned(),
            severity: Severity::Error,
        }
    }
}

pub fn lint(buffer: &TextBuffer, dialect: Dialect) -> Vec<Diagnostic> {
    let text = buffer.text();
    tokenize(&text, dialect)
        .into_iter()
        .filter(|token| token.kind == TokenKind::Unterminated)
        .map(|token| {
            let start = buffer.position_of(token.start);
            let message = if token.text.starts_with("/*") {
                "unterminated block comment"
            } else {
                "unterminated literal"
            };
            Diagnostic {
                line: start.line,
                start_column: start.column,
                end_column: start.column + 1,
                message: message.to_owned(),
                severity: Severity::Warning,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_server_position_into_buffer() {
        let buffer = TextBuffer::from_text("select 1;\nselect *\nfrm users;");
        let diagnostic = Diagnostic::from_server_error(&buffer, 10, Some(9), "syntax error");
        assert_eq!(
            (
                diagnostic.line,
                diagnostic.start_column,
                diagnostic.end_column
            ),
            (2, 0, 3)
        );
    }

    #[test]
    fn lints_unterminated_strings() {
        let buffer = TextBuffer::from_text("select 'abc");
        let diagnostics = lint(&buffer, Dialect::Generic);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].start_column, 7);
    }
}
