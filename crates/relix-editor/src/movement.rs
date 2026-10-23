/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::buffer::{Position, TextBuffer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    Word,
    Symbol,
}

fn class_of(ch: char) -> CharClass {
    if ch.is_whitespace() {
        CharClass::Space
    } else if ch.is_alphanumeric() || ch == '_' {
        CharClass::Word
    } else {
        CharClass::Symbol
    }
}

impl TextBuffer {
    fn horizontal_move(&mut self, target: Position, extend: bool) {
        self.preferred_column = None;
        self.history.seal();
        self.set_cursor(target, extend);
    }

    pub fn move_left(&mut self, extend: bool) {
        if let (Some((start, _)), false) = (self.selection(), extend) {
            return self.horizontal_move(start, false);
        }
        let Position { line, column } = self.cursor;
        let target = match (column, line) {
            (0, 0) => self.cursor,
            (0, _) => Position::new(line - 1, self.line_length(line - 1)),
            _ => Position::new(line, column - 1),
        };
        self.horizontal_move(target, extend);
    }

    pub fn move_right(&mut self, extend: bool) {
        if let (Some((_, end)), false) = (self.selection(), extend) {
            return self.horizontal_move(end, false);
        }
        let Position { line, column } = self.cursor;
        let target = if column < self.line_length(line) {
            Position::new(line, column + 1)
        } else if line + 1 < self.line_count() {
            Position::new(line + 1, 0)
        } else {
            self.cursor
        };
        self.horizontal_move(target, extend);
    }

    pub fn move_vertical(&mut self, delta: isize, extend: bool) {
        self.history.seal();
        let preferred = *self.preferred_column.get_or_insert(self.cursor.column);
        let last = self.line_count() - 1;
        let line = self.cursor.line.saturating_add_signed(delta).min(last);
        let target = if delta < 0 && self.cursor.line == 0 {
            Position::new(0, 0)
        } else if delta > 0 && self.cursor.line == last {
            Position::new(last, self.line_length(last))
        } else {
            Position::new(line, preferred.min(self.line_length(line)))
        };
        self.set_cursor(target, extend);
    }

    pub fn move_up(&mut self, extend: bool) {
        self.move_vertical(-1, extend);
    }

    pub fn move_down(&mut self, extend: bool) {
        self.move_vertical(1, extend);
    }

    /// Moves to the first non-blank character of the line, or to column 0 when the
    /// cursor already sits there ("smart home").
    pub fn move_line_start(&mut self, extend: bool) {
        let line = self.cursor.line;
        let indent = self
            .line(line)
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .count();
        let column = if self.cursor.column == indent {
            0
        } else {
            indent
        };
        self.horizontal_move(Position::new(line, column), extend);
    }

    pub fn move_line_end(&mut self, extend: bool) {
        let line = self.cursor.line;
        self.horizontal_move(Position::new(line, self.line_length(line)), extend);
    }

    pub fn move_document_start(&mut self, extend: bool) {
        self.horizontal_move(Position::default(), extend);
    }

    pub fn move_document_end(&mut self, extend: bool) {
        let last = self.line_count() - 1;
        self.horizontal_move(Position::new(last, self.line_length(last)), extend);
    }

    pub fn move_word_left(&mut self, extend: bool) {
        let target = self.word_boundary_left(self.cursor);
        self.horizontal_move(target, extend);
    }

    pub fn move_word_right(&mut self, extend: bool) {
        let target = self.word_boundary_right(self.cursor);
        self.horizontal_move(target, extend);
    }

    pub(crate) fn word_boundary_left(&self, from: Position) -> Position {
        if from.column == 0 {
            return if from.line == 0 {
                from
            } else {
                Position::new(from.line - 1, self.line_length(from.line - 1))
            };
        }
        let chars: Vec<char> = self.line(from.line).chars().collect();
        let mut column = from.column;
        while column > 0 && class_of(chars[column - 1]) == CharClass::Space {
            column -= 1;
        }
        if column > 0 {
            let class = class_of(chars[column - 1]);
            while column > 0 && class_of(chars[column - 1]) == class {
                column -= 1;
            }
        }
        Position::new(from.line, column)
    }

    pub(crate) fn word_boundary_right(&self, from: Position) -> Position {
        let chars: Vec<char> = self.line(from.line).chars().collect();
        if from.column >= chars.len() {
            return if from.line + 1 < self.line_count() {
                Position::new(from.line + 1, 0)
            } else {
                from
            };
        }
        let mut column = from.column;
        let class = class_of(chars[column]);
        while column < chars.len() && class_of(chars[column]) == class {
            column += 1;
        }
        while column < chars.len() && class_of(chars[column]) == CharClass::Space {
            column += 1;
        }
        Position::new(from.line, column)
    }

    /// The identifier-like word ending at the cursor, used as completion prefix.
    pub fn word_before_cursor(&self) -> (Position, String) {
        let chars: Vec<char> = self.line(self.cursor.line).chars().collect();
        let mut column = self.cursor.column.min(chars.len());
        while column > 0 && (chars[column - 1].is_alphanumeric() || chars[column - 1] == '_') {
            column -= 1;
        }
        let word: String = chars[column..self.cursor.column.min(chars.len())]
            .iter()
            .collect();
        (Position::new(self.cursor.line, column), word)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_moves_wrap_lines() {
        let mut buffer = TextBuffer::from_text("ab\ncd");
        buffer.set_cursor(Position::new(1, 0), false);
        buffer.move_left(false);
        assert_eq!(buffer.cursor(), Position::new(0, 2));
        buffer.move_right(false);
        assert_eq!(buffer.cursor(), Position::new(1, 0));
    }

    #[test]
    fn vertical_moves_keep_preferred_column() {
        let mut buffer = TextBuffer::from_text("long line\nx\nanother line");
        buffer.set_cursor(Position::new(0, 7), false);
        buffer.move_down(false);
        assert_eq!(buffer.cursor(), Position::new(1, 1));
        buffer.move_down(false);
        assert_eq!(buffer.cursor(), Position::new(2, 7));
        buffer.move_down(false);
        assert_eq!(buffer.cursor(), Position::new(2, 12));
    }

    #[test]
    fn word_moves_skip_classes() {
        let mut buffer = TextBuffer::from_text("select  users.name");
        buffer.move_word_right(false);
        assert_eq!(buffer.cursor().column, 8);
        buffer.move_word_right(false);
        assert_eq!(buffer.cursor().column, 13);
        buffer.move_line_end(false);
        buffer.move_word_left(false);
        assert_eq!(buffer.cursor().column, 14);
    }

    #[test]
    fn smart_home_toggles() {
        let mut buffer = TextBuffer::from_text("    where");
        buffer.move_line_end(false);
        buffer.move_line_start(false);
        assert_eq!(buffer.cursor().column, 4);
        buffer.move_line_start(false);
        assert_eq!(buffer.cursor().column, 0);
    }

    #[test]
    fn shift_moves_extend_selection() {
        let mut buffer = TextBuffer::from_text("abc");
        buffer.move_right(true);
        buffer.move_right(true);
        assert_eq!(buffer.selected_text().as_deref(), Some("ab"));
        buffer.move_left(false);
        assert_eq!(buffer.cursor(), Position::new(0, 0));
        assert!(!buffer.has_selection());
    }

    #[test]
    fn word_before_cursor_finds_prefix() {
        let mut buffer = TextBuffer::from_text("select us");
        buffer.move_line_end(false);
        assert_eq!(
            buffer.word_before_cursor(),
            (Position::new(0, 7), "us".to_owned())
        );
    }
}
