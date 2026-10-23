/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_config::{KeyChord, KeyCode};
use relix_editor::TextBuffer;

const PAGE_LINES: isize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferKey {
    Typed(char),
    Edited,
    Moved,
    Ignored,
}

/// Applies a key press to a multi-line text buffer the way common editors do: shift
/// extends the selection, ctrl moves by word or to the document edges.
pub fn apply_buffer_key(buffer: &mut TextBuffer, chord: KeyChord, tab_width: usize) -> BufferKey {
    let shift = chord.modifiers.shift;
    let ctrl = chord.modifiers.ctrl;
    let alt = chord.modifiers.alt;
    match chord.code {
        KeyCode::Char(ch) if !ctrl && !alt => {
            buffer.insert_char(ch);
            return BufferKey::Typed(ch);
        }
        KeyCode::Enter if !ctrl && !alt => buffer.insert_newline(),
        KeyCode::Backspace if ctrl || alt => buffer.delete_word_back(),
        KeyCode::Backspace => buffer.backspace(),
        KeyCode::Delete => buffer.delete_forward(),
        KeyCode::Tab => buffer.indent(tab_width),
        KeyCode::BackTab => buffer.dedent(tab_width),
        KeyCode::Left if ctrl => return moved(|| buffer.move_word_left(shift)),
        KeyCode::Right if ctrl => return moved(|| buffer.move_word_right(shift)),
        KeyCode::Left => return moved(|| buffer.move_left(shift)),
        KeyCode::Right => return moved(|| buffer.move_right(shift)),
        KeyCode::Up => return moved(|| buffer.move_up(shift)),
        KeyCode::Down => return moved(|| buffer.move_down(shift)),
        KeyCode::Home if ctrl => return moved(|| buffer.move_document_start(shift)),
        KeyCode::End if ctrl => return moved(|| buffer.move_document_end(shift)),
        KeyCode::Home => return moved(|| buffer.move_line_start(shift)),
        KeyCode::End => return moved(|| buffer.move_line_end(shift)),
        KeyCode::PageUp => return moved(|| buffer.move_vertical(-PAGE_LINES, shift)),
        KeyCode::PageDown => return moved(|| buffer.move_vertical(PAGE_LINES, shift)),
        _ => return BufferKey::Ignored,
    }
    BufferKey::Edited
}

fn moved(movement: impl FnOnce()) -> BufferKey {
    movement();
    BufferKey::Moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_config::Modifiers;

    #[test]
    fn typing_and_selection_keys() {
        let mut buffer = TextBuffer::from_text("");
        for ch in "ab".chars() {
            assert_eq!(
                apply_buffer_key(&mut buffer, KeyChord::char(ch), 4),
                BufferKey::Typed(ch)
            );
        }
        let shift_left = KeyChord::new(KeyCode::Left, Modifiers::SHIFT);
        assert_eq!(
            apply_buffer_key(&mut buffer, shift_left, 4),
            BufferKey::Moved
        );
        assert_eq!(buffer.selected_text().as_deref(), Some("b"));
        apply_buffer_key(&mut buffer, KeyChord::plain(KeyCode::Backspace), 4);
        assert_eq!(buffer.text(), "a");
        assert_eq!(
            apply_buffer_key(&mut buffer, KeyChord::plain(KeyCode::F(5)), 4),
            BufferKey::Ignored
        );
    }
}
