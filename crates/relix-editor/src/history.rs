/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::buffer::{Position, TextBuffer};

const MAX_UNDO_GROUPS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    Deleting,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRecord {
    pub start: Position,
    pub removed: String,
    pub inserted: String,
    pub cursor_before: Position,
    pub cursor_after: Position,
}

#[derive(Debug, Clone, Default)]
struct UndoGroup {
    records: Vec<EditRecord>,
    kind: Option<EditKind>,
}

#[derive(Debug, Clone, Default)]
pub struct EditHistory {
    undo: Vec<UndoGroup>,
    redo: Vec<UndoGroup>,
    sealed: bool,
}

impl EditHistory {
    /// Records an edit. Consecutive typing or deleting at the cursor is merged into one
    /// undo step; any other edit, cursor jump or explicit [`EditHistory::seal`] starts a
    /// new step.
    pub fn record(&mut self, record: EditRecord, kind: EditKind) {
        self.redo.clear();
        let continues = !self.sealed
            && kind != EditKind::Other
            && self.undo.last().is_some_and(|group| {
                group.kind == Some(kind)
                    && group
                        .records
                        .last()
                        .is_some_and(|last| last.cursor_after == record.cursor_before)
            });
        if continues {
            if let Some(group) = self.undo.last_mut() {
                group.records.push(record);
            }
        } else {
            self.undo.push(UndoGroup {
                records: vec![record],
                kind: Some(kind),
            });
            if self.undo.len() > MAX_UNDO_GROUPS {
                self.undo.remove(0);
            }
        }
        self.sealed = kind == EditKind::Other;
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn begin_group(&mut self) {
        self.undo.push(UndoGroup::default());
        self.sealed = false;
    }

    pub fn can_undo(&self) -> bool {
        self.undo.iter().any(|group| !group.records.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

impl TextBuffer {
    /// Groups several edits into one undo step, e.g. "replace all".
    pub fn transaction(&mut self, edits: impl FnOnce(&mut TextBuffer)) {
        self.history.begin_group();
        self.history.sealed = false;
        let before = self.history.undo.len();
        edits(self);
        let created: Vec<EditRecord> = self
            .history
            .undo
            .drain(before.saturating_sub(1)..)
            .flat_map(|group| group.records)
            .collect();
        if !created.is_empty() {
            self.history.undo.push(UndoGroup {
                records: created,
                kind: Some(EditKind::Other),
            });
        }
        self.history.sealed = true;
    }

    pub fn undo(&mut self) -> bool {
        let Some(group) = pop_non_empty(&mut self.history.undo) else {
            return false;
        };
        for record in group.records.iter().rev() {
            let inserted_end = end_of(record.start, &record.inserted);
            self.splice(record.start, inserted_end, &record.removed);
        }
        if let Some(first) = group.records.first() {
            self.cursor = self.clamp(first.cursor_before);
        }
        self.anchor = None;
        self.bump_version();
        self.history.redo.push(group);
        self.history.sealed = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(group) = self.history.redo.pop() else {
            return false;
        };
        for record in &group.records {
            let removed_end = end_of(record.start, &record.removed);
            self.splice(record.start, removed_end, &record.inserted);
        }
        if let Some(last) = group.records.last() {
            self.cursor = self.clamp(last.cursor_after);
        }
        self.anchor = None;
        self.bump_version();
        self.history.undo.push(group);
        self.history.sealed = true;
        true
    }
}

fn pop_non_empty(groups: &mut Vec<UndoGroup>) -> Option<UndoGroup> {
    while let Some(group) = groups.pop() {
        if !group.records.is_empty() {
            return Some(group);
        }
    }
    None
}

fn end_of(start: Position, text: &str) -> Position {
    let mut lines = text.split('\n');
    let first = lines.next().map_or(0, |line| line.chars().count());
    match lines.enumerate().last() {
        Some((index, last)) => Position::new(start.line + index + 1, last.chars().count()),
        None => Position::new(start.line, start.column + first),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_is_undone_as_one_step() {
        let mut buffer = TextBuffer::from_text("");
        for ch in "select".chars() {
            buffer.insert_char(ch);
        }
        buffer.insert_newline();
        for ch in "1".chars() {
            buffer.insert_char(ch);
        }
        assert_eq!(buffer.text(), "select\n1");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "select\n");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "select");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "");
        assert!(!buffer.undo());
        assert!(buffer.redo());
        assert!(buffer.redo());
        assert_eq!(buffer.text(), "select\n");
        assert_eq!(buffer.cursor(), Position::new(1, 0));
    }

    #[test]
    fn cursor_jump_breaks_typing_group() {
        let mut buffer = TextBuffer::from_text("ab");
        buffer.set_cursor(Position::new(0, 2), false);
        buffer.insert_char('c');
        buffer.set_cursor(Position::new(0, 0), false);
        buffer.insert_char('x');
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "abc");
    }

    #[test]
    fn transactions_undo_together() {
        let mut buffer = TextBuffer::from_text("a a a");
        buffer.transaction(|buffer| {
            buffer.replace_range(
                Position::new(0, 0),
                Position::new(0, 1),
                "b",
                EditKind::Other,
            );
            buffer.replace_range(
                Position::new(0, 4),
                Position::new(0, 5),
                "b",
                EditKind::Other,
            );
        });
        assert_eq!(buffer.text(), "b a b");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "a a a");
    }
}
