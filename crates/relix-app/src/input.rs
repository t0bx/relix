/*
 * @author t0bx
 * @since 23.10.2026
 */

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineInput {
    text: String,
    cursor: usize,
}

impl LineInput {
    pub fn with_text(text: &str) -> Self {
        let text: String = text
            .chars()
            .filter(|ch| *ch != '\n' && *ch != '\r')
            .collect();
        let cursor = text.chars().count();
        Self { text, cursor }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn set(&mut self, text: &str) {
        *self = Self::with_text(text);
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn insert_char(&mut self, ch: char) {
        if ch == '\n' || ch == '\r' {
            return;
        }
        let index = self.byte_index(self.cursor);
        self.text.insert(index, ch);
        self.cursor += 1;
    }

    pub fn insert_str(&mut self, text: &str) {
        for ch in text.chars() {
            self.insert_char(if ch == '\n' { ' ' } else { ch });
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        let index = self.byte_index(self.cursor);
        self.text.remove(index);
    }

    pub fn delete(&mut self) {
        if self.cursor < self.len() {
            let index = self.byte_index(self.cursor);
            self.text.remove(index);
        }
    }

    pub fn delete_word_back(&mut self) {
        let target = self.word_start_before(self.cursor);
        let (from, to) = (self.byte_index(target), self.byte_index(self.cursor));
        self.text.replace_range(from..to, "");
        self.cursor = target;
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len());
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.len();
    }

    pub fn move_word_left(&mut self) {
        self.cursor = self.word_start_before(self.cursor);
    }

    pub fn move_word_right(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        let mut index = self.cursor;
        while index < chars.len() && !chars[index].is_alphanumeric() {
            index += 1;
        }
        while index < chars.len() && chars[index].is_alphanumeric() {
            index += 1;
        }
        self.cursor = index;
    }

    /// Replaces the characters `start..cursor` (char indices) with `replacement`, used to
    /// accept a completion for the word being typed.
    pub fn replace_before_cursor(&mut self, start: usize, replacement: &str) {
        let start = start.min(self.cursor);
        let (from, to) = (self.byte_index(start), self.byte_index(self.cursor));
        self.text.replace_range(from..to, replacement);
        self.cursor = start + replacement.chars().count();
    }

    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn byte_cursor(&self) -> usize {
        self.byte_index(self.cursor)
    }

    fn byte_index(&self, char_index: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_index)
            .map_or(self.text.len(), |(byte, _)| byte)
    }

    fn word_start_before(&self, from: usize) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut index = from.min(chars.len());
        while index > 0 && !chars[index - 1].is_alphanumeric() {
            index -= 1;
        }
        while index > 0 && chars[index - 1].is_alphanumeric() {
            index -= 1;
        }
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_at_cursor_with_unicode() {
        let mut input = LineInput::with_text("äbc");
        input.move_left();
        input.insert_char('x');
        assert_eq!(input.text(), "äbxc");
        input.move_home();
        input.delete();
        assert_eq!(input.text(), "bxc");
        input.move_end();
        input.backspace();
        assert_eq!(input.text(), "bx");
    }

    #[test]
    fn word_operations() {
        let mut input = LineInput::with_text("status = 'ok' and na");
        input.delete_word_back();
        assert_eq!(input.text(), "status = 'ok' and ");
        input.move_word_left();
        assert_eq!(input.cursor(), 14);
        input.move_end();
        input.replace_before_cursor(14, "AND ");
        assert_eq!(input.text(), "status = 'ok' AND ");
    }

    #[test]
    fn newlines_are_ignored() {
        let mut input = LineInput::default();
        input.insert_str("a\nb");
        assert_eq!(input.text(), "a b");
    }
}
