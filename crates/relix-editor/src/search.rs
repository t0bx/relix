/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::buffer::{Position, TextBuffer};
use crate::history::EditKind;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchState {
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    matches: Vec<(Position, Position)>,
    current: Option<usize>,
}

impl SearchState {
    pub fn matches(&self) -> &[(Position, Position)] {
        &self.matches
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub fn refresh(&mut self, buffer: &TextBuffer) {
        self.matches = find_all(buffer, &self.query, self.case_sensitive);
        self.current = self.current.filter(|index| *index < self.matches.len());
    }

    /// Selects the next match at or after the cursor (wrapping around) and returns it.
    pub fn next(&mut self, buffer: &mut TextBuffer) -> Option<(Position, Position)> {
        self.refresh(buffer);
        let cursor = buffer
            .selection()
            .map_or(buffer.cursor(), |(start, _)| start);
        let after_current = self
            .current
            .and_then(|index| self.matches.get(index))
            .is_some_and(|(start, _)| *start == cursor);
        let index = self
            .matches
            .iter()
            .position(|(start, _)| {
                if after_current {
                    *start > cursor
                } else {
                    *start >= cursor
                }
            })
            .or(if self.matches.is_empty() {
                None
            } else {
                Some(0)
            })?;
        self.select(buffer, index)
    }

    pub fn previous(&mut self, buffer: &mut TextBuffer) -> Option<(Position, Position)> {
        self.refresh(buffer);
        let cursor = buffer
            .selection()
            .map_or(buffer.cursor(), |(start, _)| start);
        let index = self
            .matches
            .iter()
            .rposition(|(start, _)| *start < cursor)
            .or(self.matches.len().checked_sub(1))?;
        self.select(buffer, index)
    }

    pub fn replace_current(&mut self, buffer: &mut TextBuffer) -> bool {
        let Some((start, end)) = self
            .current
            .and_then(|index| self.matches.get(index))
            .copied()
        else {
            return false;
        };
        if buffer.selection() != Some((start, end)) {
            return false;
        }
        let after = buffer.replace_range(start, end, &self.replacement, EditKind::Other);
        buffer.set_cursor(after, false);
        self.current = None;
        self.next(buffer);
        true
    }

    pub fn replace_all(&mut self, buffer: &mut TextBuffer) -> usize {
        self.refresh(buffer);
        let targets: Vec<_> = self.matches.iter().rev().copied().collect();
        let count = targets.len();
        if count > 0 {
            let replacement = self.replacement.clone();
            buffer.transaction(|buffer| {
                for (start, end) in targets {
                    buffer.replace_range(start, end, &replacement, EditKind::Other);
                }
            });
        }
        self.refresh(buffer);
        count
    }

    fn select(&mut self, buffer: &mut TextBuffer, index: usize) -> Option<(Position, Position)> {
        let found = *self.matches.get(index)?;
        self.current = Some(index);
        buffer.select_range(found.0, found.1);
        Some(found)
    }
}

pub fn find_all(
    buffer: &TextBuffer,
    query: &str,
    case_sensitive: bool,
) -> Vec<(Position, Position)> {
    if query.is_empty() {
        return Vec::new();
    }
    let fold = |text: &str| {
        if case_sensitive {
            text.to_owned()
        } else {
            text.to_lowercase()
        }
    };
    let needle = fold(query);
    let needle_chars = query.chars().count();
    let mut found = Vec::new();
    for (line_index, line) in buffer.lines().iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let haystack: Vec<String> = chars.iter().map(|ch| fold(&ch.to_string())).collect();
        let mut column = 0;
        while column + needle_chars <= chars.len() {
            let window: String = haystack[column..column + needle_chars].concat();
            if window == needle {
                found.push((
                    Position::new(line_index, column),
                    Position::new(line_index, column + needle_chars),
                ));
                column += needle_chars.max(1);
            } else {
                column += 1;
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_cycles_matches() {
        let mut buffer = TextBuffer::from_text("select a\nfrom A where a");
        let mut search = SearchState {
            query: "a".into(),
            ..SearchState::default()
        };
        search.refresh(&buffer);
        assert_eq!(search.matches().len(), 3);
        assert_eq!(
            search.next(&mut buffer),
            Some((Position::new(0, 7), Position::new(0, 8)))
        );
        assert_eq!(search.next(&mut buffer).unwrap().0, Position::new(1, 5));
        assert_eq!(search.next(&mut buffer).unwrap().0, Position::new(1, 13));
        assert_eq!(search.next(&mut buffer).unwrap().0, Position::new(0, 7));
        assert_eq!(
            search.previous(&mut buffer).unwrap().0,
            Position::new(1, 13)
        );
        search.case_sensitive = true;
        search.refresh(&buffer);
        assert_eq!(search.matches().len(), 2);
    }

    #[test]
    fn replaces_current_and_all_with_single_undo() {
        let mut buffer = TextBuffer::from_text("foo foo foo");
        let mut search = SearchState {
            query: "foo".into(),
            replacement: "bar".into(),
            ..SearchState::default()
        };
        search.next(&mut buffer);
        assert!(search.replace_current(&mut buffer));
        assert_eq!(buffer.text(), "bar foo foo");
        assert_eq!(search.replace_all(&mut buffer), 2);
        assert_eq!(buffer.text(), "bar bar bar");
        buffer.undo();
        assert_eq!(buffer.text(), "bar foo foo");
    }
}
