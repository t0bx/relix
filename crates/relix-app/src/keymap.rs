/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::command::{COMMANDS, Command, KeyContext, LEADER_GLOBAL, LEADER_NON_TEXT, spec_by_id};
use relix_config::settings::KeyBindingValue;
use relix_config::{KeyChord, KeySequence};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyResolution {
    Command(Command),
    Pending,
    NoMatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Continuation {
    pub key: KeyChord,
    pub command: Command,
}

#[derive(Debug, Clone, Default)]
pub struct Keymap {
    bindings: HashMap<KeyContext, Vec<(KeySequence, Command)>>,
}

impl Keymap {
    pub fn defaults() -> Self {
        Self::with_overrides(&BTreeMap::new()).0
    }

    /// Builds the keymap from the command registry. A user override for a command id
    /// replaces that command's default keys (an empty list unbinds it); leader-key
    /// shortcuts are kept. Unknown command ids are reported as warnings.
    pub fn with_overrides(overrides: &BTreeMap<String, KeyBindingValue>) -> (Self, Vec<String>) {
        let mut keymap = Self::default();
        let mut warnings = Vec::new();
        for id in overrides.keys() {
            if spec_by_id(id).is_none() {
                warnings.push(format!("unknown command '{id}' in [keybindings]"));
            }
        }
        for spec in COMMANDS {
            let keys: Vec<&str> = match overrides.get(spec.id) {
                Some(value) => value.specs(),
                None => spec.keys.to_vec(),
            };
            for key in keys {
                keymap.bind(spec.context, key, spec.command);
            }
            if let Some(leader) = spec.leader {
                keymap.bind(
                    KeyContext::Global,
                    &format!("{LEADER_GLOBAL} {leader}"),
                    spec.command,
                );
                keymap.bind(
                    KeyContext::NonText,
                    &format!("{LEADER_NON_TEXT} {leader}"),
                    spec.command,
                );
            }
        }
        (keymap, warnings)
    }

    fn bind(&mut self, context: KeyContext, key: &str, command: Command) {
        if let Ok(sequence) = key.parse::<KeySequence>() {
            self.bindings
                .entry(context)
                .or_default()
                .push((sequence, command));
        }
    }

    /// Resolves `pending + chord` against the contexts in priority order. The first
    /// context that knows the sequence decides: an exact binding runs its command, a
    /// longer binding sharing the prefix keeps the sequence pending (which-key).
    pub fn resolve(
        &self,
        contexts: &[KeyContext],
        pending: &[KeyChord],
        chord: KeyChord,
    ) -> KeyResolution {
        let mut sequence = pending.to_vec();
        sequence.push(chord);
        for context in contexts {
            let Some(bindings) = self.bindings.get(context) else {
                continue;
            };
            if let Some((_, command)) = bindings.iter().find(|(keys, _)| keys.chords() == sequence)
            {
                return KeyResolution::Command(*command);
            }
            if bindings.iter().any(|(keys, _)| {
                keys.chords().len() > sequence.len() && keys.starts_with(&sequence)
            }) {
                return KeyResolution::Pending;
            }
        }
        KeyResolution::NoMatch
    }

    pub fn continuations(&self, contexts: &[KeyContext], prefix: &[KeyChord]) -> Vec<Continuation> {
        let mut found: Vec<Continuation> = Vec::new();
        for context in contexts {
            for (keys, command) in self.bindings.get(context).into_iter().flatten() {
                let chords = keys.chords();
                if chords.len() == prefix.len() + 1
                    && keys.starts_with(prefix)
                    && !found
                        .iter()
                        .any(|existing| existing.key == chords[prefix.len()])
                {
                    found.push(Continuation {
                        key: chords[prefix.len()],
                        command: *command,
                    });
                }
            }
        }
        found
    }

    pub fn keys_for(&self, command: Command) -> Vec<KeySequence> {
        let mut keys: Vec<KeySequence> = self
            .bindings
            .values()
            .flatten()
            .filter(|(_, bound)| *bound == command)
            .map(|(keys, _)| keys.clone())
            .collect();
        keys.sort_by_key(|keys| (keys.chords().len(), keys.to_string()));
        keys.dedup();
        keys
    }

    pub fn primary_key(&self, command: Command) -> Option<String> {
        self.keys_for(command).first().map(ToString::to_string)
    }

    pub fn bindings_in(&self, context: KeyContext) -> Vec<(KeySequence, Command)> {
        self.bindings.get(&context).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_config::KeyCode;

    fn chord(text: &str) -> KeyChord {
        text.parse().unwrap()
    }

    #[test]
    fn context_priority_decides() {
        let keymap = Keymap::defaults();
        let grid = [
            KeyContext::Grid,
            KeyContext::Navigable,
            KeyContext::NonText,
            KeyContext::Global,
        ];
        assert_eq!(
            keymap.resolve(&grid, &[], chord("ctrl+s")),
            KeyResolution::Command(Command::ReviewChanges)
        );
        let editor = [KeyContext::Editor, KeyContext::Global];
        assert_eq!(
            keymap.resolve(&editor, &[], chord("ctrl+s")),
            KeyResolution::Command(Command::Save)
        );
        assert_eq!(
            keymap.resolve(&editor, &[], chord("j")),
            KeyResolution::NoMatch
        );
        assert_eq!(
            keymap.resolve(&grid, &[], chord("j")),
            KeyResolution::Command(Command::MoveDown)
        );
    }

    #[test]
    fn leader_sequences_are_pending_then_resolve() {
        let keymap = Keymap::defaults();
        let explorer = [
            KeyContext::Explorer,
            KeyContext::Navigable,
            KeyContext::NonText,
            KeyContext::Global,
        ];
        let space = KeyChord::plain(KeyCode::Char(' '));
        assert_eq!(
            keymap.resolve(&explorer, &[], space),
            KeyResolution::Pending
        );
        assert_eq!(
            keymap.resolve(&explorer, &[space], chord("v")),
            KeyResolution::Command(Command::SplitVertical)
        );
        let continuations = keymap.continuations(&explorer, &[space]);
        assert!(
            continuations
                .iter()
                .any(|entry| entry.command == Command::OpenProjects)
        );
        let editor = [KeyContext::Editor, KeyContext::Global];
        assert_eq!(
            keymap.resolve(&editor, &[], chord("ctrl+k")),
            KeyResolution::Pending
        );
    }

    #[test]
    fn overrides_replace_defaults() {
        let mut overrides = BTreeMap::new();
        overrides.insert("palette.open".to_owned(), KeyBindingValue::One("f2".into()));
        overrides.insert("nope".to_owned(), KeyBindingValue::Many(vec![]));
        let (keymap, warnings) = Keymap::with_overrides(&overrides);
        assert_eq!(warnings.len(), 1);
        let global = [KeyContext::Global];
        assert_eq!(
            keymap.resolve(&global, &[], chord("ctrl+p")),
            KeyResolution::NoMatch
        );
        assert_eq!(
            keymap.resolve(&global, &[], chord("f2")),
            KeyResolution::Command(Command::OpenPalette)
        );
        assert_eq!(
            keymap.primary_key(Command::OpenPalette).as_deref(),
            Some("F2")
        );
    }
}
