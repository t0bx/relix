/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ConfigError;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyCode {
    Char(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Insert,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
    };
    pub const CTRL: Self = Self {
        ctrl: true,
        alt: false,
        shift: false,
    };
    pub const ALT: Self = Self {
        ctrl: false,
        alt: true,
        shift: false,
    };
    pub const SHIFT: Self = Self {
        ctrl: false,
        alt: false,
        shift: true,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyChord {
    pub code: KeyCode,
    pub modifiers: Modifiers,
}

impl KeyChord {
    /// Creates a chord in canonical form: shift is implied by the case of printable
    /// characters, so `Char('G')` never carries an explicit shift flag.
    pub fn new(code: KeyCode, modifiers: Modifiers) -> Self {
        let mut modifiers = modifiers;
        let code = match code {
            KeyCode::Char(ch) if modifiers.shift && ch.is_ascii_lowercase() => {
                KeyCode::Char(ch.to_ascii_uppercase())
            }
            KeyCode::BackTab => {
                modifiers.shift = false;
                KeyCode::BackTab
            }
            other => other,
        };
        if matches!(code, KeyCode::Char(_)) {
            modifiers.shift = false;
        }
        Self { code, modifiers }
    }

    pub fn plain(code: KeyCode) -> Self {
        Self::new(code, Modifiers::NONE)
    }

    pub fn ctrl(ch: char) -> Self {
        Self::new(KeyCode::Char(ch), Modifiers::CTRL)
    }

    pub fn char(ch: char) -> Self {
        Self::new(KeyCode::Char(ch), Modifiers::NONE)
    }

    pub fn is_text_input(&self) -> bool {
        matches!(self.code, KeyCode::Char(_)) && !self.modifiers.ctrl && !self.modifiers.alt
    }
}

impl FromStr for KeyChord {
    type Err = ConfigError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || ConfigError::InvalidKey(text.to_owned());
        if text.is_empty() {
            return Err(invalid());
        }
        let parts: Vec<&str> = if text == "+" {
            vec!["+"]
        } else if let Some(prefix) = text.strip_suffix("++") {
            prefix.split('+').chain(std::iter::once("+")).collect()
        } else {
            text.split('+').collect()
        };
        let (key, modifier_names) = parts.split_last().ok_or_else(invalid)?;
        let mut modifiers = Modifiers::NONE;
        for name in modifier_names {
            match name.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "c" => modifiers.ctrl = true,
                "alt" | "meta" | "m" | "a" => modifiers.alt = true,
                "shift" | "s" => modifiers.shift = true,
                _ => return Err(invalid()),
            }
        }
        let code = parse_key_code(key).ok_or_else(invalid)?;
        Ok(Self::new(code, modifiers))
    }
}

fn parse_key_code(key: &str) -> Option<KeyCode> {
    let mut chars = key.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        return Some(KeyCode::Char(ch));
    }
    let lower = key.to_ascii_lowercase();
    let code = match lower.as_str() {
        "enter" | "return" | "cr" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "backspace" | "bs" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "pgup" => KeyCode::PageUp,
        "pagedown" | "pgdn" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        "plus" => KeyCode::Char('+'),
        "minus" => KeyCode::Char('-'),
        other => {
            let number = other.strip_prefix('f')?.parse::<u8>().ok()?;
            if (1..=24).contains(&number) {
                KeyCode::F(number)
            } else {
                return None;
            }
        }
    };
    Some(code)
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.modifiers.alt {
            f.write_str("Alt+")?;
        }
        if self.modifiers.shift {
            f.write_str("Shift+")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("Space"),
            KeyCode::Char(ch) if self.modifiers.ctrl || self.modifiers.alt => {
                write!(f, "{}", ch.to_ascii_uppercase())
            }
            KeyCode::Char(ch) => write!(f, "{ch}"),
            KeyCode::Enter => f.write_str("Enter"),
            KeyCode::Esc => f.write_str("Esc"),
            KeyCode::Tab => f.write_str("Tab"),
            KeyCode::BackTab => f.write_str("Shift+Tab"),
            KeyCode::Backspace => f.write_str("Backspace"),
            KeyCode::Delete => f.write_str("Del"),
            KeyCode::Insert => f.write_str("Ins"),
            KeyCode::Up => f.write_str("↑"),
            KeyCode::Down => f.write_str("↓"),
            KeyCode::Left => f.write_str("←"),
            KeyCode::Right => f.write_str("→"),
            KeyCode::Home => f.write_str("Home"),
            KeyCode::End => f.write_str("End"),
            KeyCode::PageUp => f.write_str("PgUp"),
            KeyCode::PageDown => f.write_str("PgDn"),
            KeyCode::F(number) => write!(f, "F{number}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeySequence(pub Vec<KeyChord>);

impl KeySequence {
    pub fn single(chord: KeyChord) -> Self {
        Self(vec![chord])
    }

    pub fn chords(&self) -> &[KeyChord] {
        &self.0
    }

    pub fn starts_with(&self, prefix: &[KeyChord]) -> bool {
        self.0.starts_with(prefix)
    }
}

impl FromStr for KeySequence {
    type Err = ConfigError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let chords = text
            .split_whitespace()
            .map(KeyChord::from_str)
            .collect::<Result<Vec<_>, _>>()?;
        if chords.is_empty() {
            return Err(ConfigError::InvalidKey(text.to_owned()));
        }
        Ok(Self(chords))
    }
}

impl fmt::Display for KeySequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, chord) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{chord}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifier_combinations() {
        let chord: KeyChord = "ctrl+shift+f".parse().unwrap();
        assert_eq!(chord, KeyChord::new(KeyCode::Char('F'), Modifiers::CTRL));
        let enter: KeyChord = "Ctrl+Enter".parse().unwrap();
        assert_eq!(enter.code, KeyCode::Enter);
        assert!(enter.modifiers.ctrl);
        assert_eq!("f5".parse::<KeyChord>().unwrap().code, KeyCode::F(5));
        assert_eq!("ctrl++".parse::<KeyChord>().unwrap(), KeyChord::ctrl('+'));
    }

    #[test]
    fn shifted_letters_are_canonical() {
        let shifted = KeyChord::new(KeyCode::Char('g'), Modifiers::SHIFT);
        assert_eq!(shifted, KeyChord::char('G'));
        assert!("shift+tab".parse::<KeyChord>().unwrap().modifiers.shift);
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!("hyper+x".parse::<KeyChord>().is_err());
        assert!("f99".parse::<KeyChord>().is_err());
        assert!("".parse::<KeySequence>().is_err());
    }

    #[test]
    fn sequences_parse_and_display() {
        let sequence: KeySequence = "space f".parse().unwrap();
        assert_eq!(sequence.chords().len(), 2);
        assert_eq!(sequence.to_string(), "Space f");
        assert_eq!(
            "ctrl+p".parse::<KeySequence>().unwrap().to_string(),
            "Ctrl+P"
        );
    }
}
