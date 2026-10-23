/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::buffer::{Position, TextBuffer};
use crate::history::EditKind;

impl TextBuffer {
    fn replace_selection_or_insert(&mut self, text: &str, kind: EditKind) {
        let (start, end) = self.selection().unwrap_or((self.cursor, self.cursor));
        let cursor = self.replace_range(start, end, text, kind);
        self.anchor = None;
        self.cursor = cursor;
        self.preferred_column = None;
    }

    pub fn insert_char(&mut self, ch: char) {
        let kind = if self.has_selection() {
            EditKind::Other
        } else {
            EditKind::Typing
        };
        self.replace_selection_or_insert(ch.encode_utf8(&mut [0; 4]), kind);
    }

    pub fn insert_text(&mut self, text: &str) {
        self.replace_selection_or_insert(text, EditKind::Other);
    }

    pub fn set_text(&mut self, text: &str) {
        self.select_all();
        self.insert_text(text);
        self.cursor = Position::default();
    }

    pub fn insert_newline(&mut self) {
        let indent: String = self
            .line(self.cursor.line)
            .chars()
            .take_while(|ch| *ch == ' ' || *ch == '\t')
            .collect();
        self.replace_selection_or_insert(&format!("\n{indent}"), EditKind::Other);
    }

    pub fn backspace(&mut self) {
        if self.has_selection() {
            return self.delete_selection();
        }
        if self.cursor == Position::default() {
            return;
        }
        let start = if self.cursor.column == 0 {
            Position::new(self.cursor.line - 1, self.line_length(self.cursor.line - 1))
        } else {
            Position::new(self.cursor.line, self.cursor.column - 1)
        };
        self.cursor = self.replace_range(start, self.cursor, "", EditKind::Deleting);
        self.preferred_column = None;
    }

    pub fn delete_forward(&mut self) {
        if self.has_selection() {
            return self.delete_selection();
        }
        let end = if self.cursor.column < self.line_length(self.cursor.line) {
            Position::new(self.cursor.line, self.cursor.column + 1)
        } else if self.cursor.line + 1 < self.line_count() {
            Position::new(self.cursor.line + 1, 0)
        } else {
            return;
        };
        self.replace_range(self.cursor, end, "", EditKind::Other);
    }

    pub fn delete_word_back(&mut self) {
        if self.has_selection() {
            return self.delete_selection();
        }
        let start = self.word_boundary_left(self.cursor);
        self.cursor = self.replace_range(start, self.cursor, "", EditKind::Other);
    }

    pub fn delete_selection(&mut self) {
        if let Some((start, end)) = self.selection() {
            self.cursor = self.replace_range(start, end, "", EditKind::Other);
            self.anchor = None;
        }
    }

    pub fn cut_selection(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.delete_selection();
        Some(text)
    }

    /// Tab key: indents every selected line when the selection spans lines, otherwise
    /// inserts spaces up to the next tab stop.
    pub fn indent(&mut self, tab_width: usize) {
        let tab_width = tab_width.max(1);
        match self.selection() {
            Some((start, end)) if start.line != end.line => {
                let padding = " ".repeat(tab_width);
                self.transform_lines(start.line, end.line, |line| format!("{padding}{line}"));
            }
            _ => {
                let spaces = tab_width - self.cursor.column % tab_width;
                self.replace_selection_or_insert(&" ".repeat(spaces), EditKind::Other);
            }
        }
    }

    pub fn dedent(&mut self, tab_width: usize) {
        let (first, last) = self
            .selection()
            .map_or((self.cursor.line, self.cursor.line), |(start, end)| {
                (start.line, end.line)
            });
        self.transform_lines(first, last, |line| {
            let removable = line
                .chars()
                .take(tab_width.max(1))
                .take_while(|ch| *ch == ' ')
                .count();
            line[removable..].to_owned()
        });
    }

    /// Comments the touched lines with `prefix` unless all of them are already
    /// commented, in which case the prefix is removed.
    pub fn toggle_line_comment(&mut self, prefix: &str) {
        let (first, last) = self
            .selection()
            .map_or((self.cursor.line, self.cursor.line), |(start, end)| {
                (start.line, end.line)
            });
        let all_commented = (first..=last)
            .map(|index| self.line(index).trim_start())
            .filter(|line| !line.is_empty())
            .all(|line| line.starts_with(prefix));
        let marker = format!("{prefix} ");
        self.transform_lines(first, last, |line| {
            let indent = line.len() - line.trim_start().len();
            let (head, body) = line.split_at(indent);
            if all_commented {
                let stripped = body
                    .strip_prefix(&marker)
                    .or_else(|| body.strip_prefix(prefix))
                    .unwrap_or(body);
                format!("{head}{stripped}")
            } else if body.is_empty() {
                line.to_owned()
            } else {
                format!("{head}{marker}{body}")
            }
        });
    }

    pub fn duplicate_line(&mut self) {
        let line = self.cursor.line;
        let text = format!("\n{}", self.line(line));
        let end = Position::new(line, self.line_length(line));
        self.replace_range(end, end, &text, EditKind::Other);
        self.cursor = Position::new(line + 1, self.cursor.column);
    }

    pub fn delete_line(&mut self) {
        let line = self.cursor.line;
        let (start, end) = if line + 1 < self.line_count() {
            (Position::new(line, 0), Position::new(line + 1, 0))
        } else if line > 0 {
            (
                Position::new(line - 1, self.line_length(line - 1)),
                Position::new(line, self.line_length(line)),
            )
        } else {
            (Position::new(0, 0), Position::new(0, self.line_length(0)))
        };
        self.replace_range(start, end, "", EditKind::Other);
        self.anchor = None;
        self.cursor = self.clamp(Position::new(line, self.cursor.column));
    }

    fn transform_lines(&mut self, first: usize, last: usize, transform: impl Fn(&str) -> String) {
        let start = Position::new(first, 0);
        let end = Position::new(last, self.line_length(last));
        let replaced: Vec<String> = (first..=last)
            .map(|index| transform(self.line(index)))
            .collect();
        let new_end = self.replace_range(start, end, &replaced.join("\n"), EditKind::Other);
        self.anchor = Some(start);
        self.cursor = new_end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn newline_keeps_indentation() {
        let mut buffer = TextBuffer::from_text("  select");
        buffer.move_line_end(false);
        buffer.insert_newline();
        buffer.insert_char('x');
        assert_eq!(buffer.text(), "  select\n  x");
    }

    #[test]
    fn backspace_joins_lines_and_deletes_selection() {
        let mut buffer = TextBuffer::from_text("ab\ncd");
        buffer.set_cursor(Position::new(1, 0), false);
        buffer.backspace();
        assert_eq!(buffer.text(), "abcd");
        buffer.select_all();
        buffer.backspace();
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn typing_replaces_selection() {
        let mut buffer = TextBuffer::from_text("select 1");
        buffer.set_cursor(Position::new(0, 7), false);
        buffer.set_cursor(Position::new(0, 8), true);
        buffer.insert_char('2');
        assert_eq!(buffer.text(), "select 2");
    }

    #[test]
    fn indent_and_dedent_selected_lines() {
        let mut buffer = TextBuffer::from_text("a\nb");
        buffer.select_all();
        buffer.indent(2);
        assert_eq!(buffer.text(), "  a\n  b");
        buffer.dedent(4);
        assert_eq!(buffer.text(), "a\nb");
        buffer.set_cursor(Position::new(0, 1), false);
        buffer.indent(4);
        assert_eq!(buffer.text(), "a   \nb");
    }

    #[test]
    fn toggles_line_comments() {
        let mut buffer = TextBuffer::from_text("  select 1\n\nfrom t");
        buffer.select_all();
        buffer.toggle_line_comment("--");
        assert_eq!(buffer.text(), "  -- select 1\n\n-- from t");
        buffer.toggle_line_comment("--");
        assert_eq!(buffer.text(), "  select 1\n\nfrom t");
    }

    #[test]
    fn line_operations() {
        let mut buffer = TextBuffer::from_text("one\ntwo");
        buffer.duplicate_line();
        assert_eq!(buffer.text(), "one\none\ntwo");
        buffer.delete_line();
        assert_eq!(buffer.text(), "one\ntwo");
        buffer.move_document_end(false);
        buffer.delete_line();
        assert_eq!(buffer.text(), "one");
        let cut = {
            buffer.select_all();
            buffer.cut_selection()
        };
        assert_eq!(cut.as_deref(), Some("one"));
        assert_eq!(buffer.text(), "");
    }
}
