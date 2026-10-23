/*
 * @author t0bx
 * @since 23.10.2026
 */

use crossterm::event::{
    Event, KeyCode as CtKey, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
};
use relix_app::events::{InputEvent, MouseInput, MouseKind};
use relix_config::{KeyChord, KeyCode, Modifiers};

pub fn map_event(event: Event) -> Option<InputEvent> {
    match event {
        Event::Key(key) => map_key(key).map(InputEvent::Key),
        Event::Paste(text) => Some(InputEvent::Paste(text)),
        Event::Mouse(mouse) => {
            let kind = match mouse.kind {
                MouseEventKind::Down(_) => MouseKind::Click,
                MouseEventKind::ScrollUp => MouseKind::ScrollUp,
                MouseEventKind::ScrollDown => MouseKind::ScrollDown,
                _ => return None,
            };
            Some(InputEvent::Mouse(MouseInput {
                kind,
                column: mouse.column,
                row: mouse.row,
            }))
        }
        Event::Resize(_, _) => Some(InputEvent::Tick),
        _ => None,
    }
}

/// Converts a crossterm key event into the terminal-independent chord used by the
/// keymap. Key releases are ignored; `Ctrl+Space` arrives as NUL on some terminals.
pub fn map_key(key: KeyEvent) -> Option<KeyChord> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let modifiers = Modifiers {
        ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
        alt: key.modifiers.contains(KeyModifiers::ALT),
        shift: key.modifiers.contains(KeyModifiers::SHIFT),
    };
    let code = match key.code {
        CtKey::Char('\0') => return Some(KeyChord::new(KeyCode::Char(' '), Modifiers::CTRL)),
        CtKey::Char(ch) => KeyCode::Char(ch),
        CtKey::Enter => KeyCode::Enter,
        CtKey::Esc => KeyCode::Esc,
        CtKey::Tab => KeyCode::Tab,
        CtKey::BackTab => KeyCode::BackTab,
        CtKey::Backspace => KeyCode::Backspace,
        CtKey::Delete => KeyCode::Delete,
        CtKey::Insert => KeyCode::Insert,
        CtKey::Up => KeyCode::Up,
        CtKey::Down => KeyCode::Down,
        CtKey::Left => KeyCode::Left,
        CtKey::Right => KeyCode::Right,
        CtKey::Home => KeyCode::Home,
        CtKey::End => KeyCode::End,
        CtKey::PageUp => KeyCode::PageUp,
        CtKey::PageDown => KeyCode::PageDown,
        CtKey::F(number) => KeyCode::F(number),
        _ => return None,
    };
    Some(KeyChord::new(code, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: CtKey, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn maps_common_chords() {
        assert_eq!(
            map_key(key(CtKey::Char('p'), KeyModifiers::CONTROL)),
            Some(KeyChord::ctrl('p'))
        );
        assert_eq!(
            map_key(key(CtKey::Char('G'), KeyModifiers::SHIFT)),
            Some(KeyChord::char('G'))
        );
        assert_eq!(
            map_key(key(CtKey::Char('\0'), KeyModifiers::NONE)),
            Some(KeyChord::new(KeyCode::Char(' '), Modifiers::CTRL))
        );
        let redo = map_key(key(
            CtKey::Char('Z'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        assert_eq!(redo, Some("ctrl+shift+z".parse().unwrap()));
        let ctrl_enter = map_key(key(CtKey::Enter, KeyModifiers::CONTROL)).unwrap();
        assert_eq!(ctrl_enter, "ctrl+enter".parse().unwrap());
    }

    #[test]
    fn ignores_key_release() {
        let mut release = key(CtKey::Char('a'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert_eq!(map_key(release), None);
    }
}
