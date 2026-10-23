/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::history::{EditHistory, EditKind, EditRecord};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Debug, Clone)]
pub struct TextBuffer {
    pub(crate) lines: Vec<String>,
    pub(crate) cursor: Position,
    pub(crate) anchor: Option<Position>,
    pub(crate) preferred_column: Option<usize>,
    pub(crate) history: EditHistory,
    version: u64,
    saved_version: u64,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::from_text("")
    }
}

impl TextBuffer {
    pub fn from_text(text: &str) -> Self {
        let normalized = text.replace("\r\n", "\n");
        Self {
            lines: normalized.split('\n').map(str::to_owned).collect(),
            cursor: Position::default(),
            anchor: None,
            preferred_column: None,
            history: EditHistory::default(),
            version: 0,
            saved_version: 0,
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn line(&self, index: usize) -> &str {
        self.lines.get(index).map_or("", String::as_str)
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line_length(&self, index: usize) -> usize {
        self.line(index).chars().count()
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_modified(&self) -> bool {
        self.version != self.saved_version
    }

    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
    }

    pub fn cursor(&self) -> Position {
        self.cursor
    }

    pub fn set_cursor(&mut self, position: Position, extend_selection: bool) {
        let position = self.clamp(position);
        if extend_selection {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = position;
        if self.anchor == Some(self.cursor) {
            self.anchor = None;
        }
    }

    pub fn selection(&self) -> Option<(Position, Position)> {
        let anchor = self.anchor?;
        Some(if anchor <= self.cursor {
            (anchor, self.cursor)
        } else {
            (self.cursor, anchor)
        })
    }

    pub fn has_selection(&self) -> bool {
        self.selection().is_some()
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(Position::default());
        let last = self.lines.len() - 1;
        self.cursor = Position::new(last, self.line_length(last));
    }

    pub fn select_range(&mut self, start: Position, end: Position) {
        self.anchor = Some(self.clamp(start));
        self.cursor = self.clamp(end);
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        Some(self.text_between(start, end))
    }

    pub fn text_between(&self, start: Position, end: Position) -> String {
        let text = self.text();
        let (from, to) = (self.offset_of(start), self.offset_of(end));
        text.get(from.min(to)..to.max(from))
            .unwrap_or_default()
            .to_owned()
    }

    pub fn clamp(&self, position: Position) -> Position {
        let line = position.line.min(self.lines.len() - 1);
        Position::new(line, position.column.min(self.line_length(line)))
    }

    /// Converts a position into a byte offset of [`TextBuffer::text`].
    pub fn offset_of(&self, position: Position) -> usize {
        let position = self.clamp(position);
        let preceding: usize = self.lines[..position.line]
            .iter()
            .map(|line| line.len() + 1)
            .sum();
        preceding + byte_index(&self.lines[position.line], position.column)
    }

    /// Converts a byte offset of [`TextBuffer::text`] back into a position, clamping
    /// offsets that fall inside a multi-byte character to its start.
    pub fn position_of(&self, offset: usize) -> Position {
        let mut remaining = offset;
        for (index, line) in self.lines.iter().enumerate() {
            if remaining <= line.len() {
                let column = line
                    .char_indices()
                    .take_while(|(byte, _)| *byte < remaining)
                    .count();
                return Position::new(index, column);
            }
            remaining -= line.len() + 1;
        }
        let last = self.lines.len() - 1;
        Position::new(last, self.line_length(last))
    }

    /// The single editing primitive: replaces the text between two positions, records the
    /// change for undo and returns the position right after the inserted text. Every
    /// higher level edit goes through here so undo/redo stays consistent.
    pub fn replace_range(
        &mut self,
        start: Position,
        end: Position,
        text: &str,
        kind: EditKind,
    ) -> Position {
        let (start, end) = {
            let (a, b) = (self.clamp(start), self.clamp(end));
            if a <= b { (a, b) } else { (b, a) }
        };
        let removed = self.text_between(start, end);
        let cursor_before = self.cursor;
        let inserted_end = self.splice(start, end, text);
        self.history.record(
            EditRecord {
                start,
                removed,
                inserted: text.replace("\r\n", "\n"),
                cursor_before,
                cursor_after: inserted_end,
            },
            kind,
        );
        self.version += 1;
        inserted_end
    }

    pub(crate) fn splice(&mut self, start: Position, end: Position, text: &str) -> Position {
        let text = text.replace("\r\n", "\n");
        let prefix: String = {
            let line = &self.lines[start.line];
            line[..byte_index(line, start.column)].to_owned()
        };
        let suffix: String = {
            let line = &self.lines[end.line];
            line[byte_index(line, end.column)..].to_owned()
        };
        let mut new_lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let last_index = new_lines.len() - 1;
        let end_column = if last_index == 0 {
            start.column + new_lines[0].chars().count()
        } else {
            new_lines[last_index].chars().count()
        };
        new_lines[0].insert_str(0, &prefix);
        new_lines[last_index].push_str(&suffix);
        self.lines.splice(start.line..=end.line, new_lines);
        Position::new(start.line + last_index, end_column)
    }

    pub(crate) fn bump_version(&mut self) {
        self.version += 1;
    }
}

pub fn byte_index(line: &str, column: usize) -> usize {
    line.char_indices()
        .nth(column)
        .map_or(line.len(), |(byte, _)| byte)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn replace_range_handles_multiline_edits() {
        let mut buffer = TextBuffer::from_text("select *\nfrom users\nwhere 1");
        let end = buffer.replace_range(
            Position::new(0, 7),
            Position::new(1, 4),
            "id,\n  name\nFROM",
            EditKind::Other,
        );
        assert_eq!(buffer.text(), "select id,\n  name\nFROM users\nwhere 1");
        assert_eq!(end, Position::new(2, 4));
        assert!(buffer.is_modified());
    }

    #[test]
    fn offsets_and_positions_roundtrip_with_unicode() {
        let buffer = TextBuffer::from_text("äb\nc€d");
        for position in [
            Position::new(0, 1),
            Position::new(1, 2),
            Position::new(1, 3),
        ] {
            assert_eq!(buffer.position_of(buffer.offset_of(position)), position);
        }
        assert_eq!(buffer.offset_of(Position::new(1, 0)), 4);
        assert_eq!(buffer.clamp(Position::new(9, 9)), Position::new(1, 3));
    }

    #[test]
    fn selection_is_ordered_and_extractable() {
        let mut buffer = TextBuffer::from_text("hello world");
        buffer.set_cursor(Position::new(0, 11), false);
        buffer.set_cursor(Position::new(0, 6), true);
        assert_eq!(
            buffer.selection(),
            Some((Position::new(0, 6), Position::new(0, 11)))
        );
        assert_eq!(buffer.selected_text().as_deref(), Some("world"));
        buffer.set_cursor(Position::new(0, 0), false);
        assert!(!buffer.has_selection());
    }

    #[test]
    fn crlf_input_is_normalized() {
        assert_eq!(TextBuffer::from_text("a\r\nb").line_count(), 2);
    }
}
