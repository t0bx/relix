/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::app::edit_line;
use crate::input::LineInput;
use relix_config::{KeyChord, KeyCode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    Text(LineInput),
    Secret(LineInput),
    Choice {
        options: Vec<String>,
        selected: usize,
    },
    Toggle(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField<Id> {
    pub id: Id,
    pub label: &'static str,
    pub value: FieldValue,
}

impl<Id> FormField<Id> {
    pub fn text(id: Id, label: &'static str, value: &str) -> Self {
        Self {
            id,
            label,
            value: FieldValue::Text(LineInput::with_text(value)),
        }
    }

    pub fn secret(id: Id, label: &'static str) -> Self {
        Self {
            id,
            label,
            value: FieldValue::Secret(LineInput::default()),
        }
    }

    pub fn choice(id: Id, label: &'static str, options: Vec<String>, selected: usize) -> Self {
        let selected = selected.min(options.len().saturating_sub(1));
        Self {
            id,
            label,
            value: FieldValue::Choice { options, selected },
        }
    }

    pub fn toggle(id: Id, label: &'static str, value: bool) -> Self {
        Self {
            id,
            label,
            value: FieldValue::Toggle(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormStatus {
    pub message: String,
    pub success: bool,
}

impl FormStatus {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            success: true,
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            success: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormCommand<Id> {
    Cancel,
    Submit,
    Test,
    Shortcut(char),
    Edited(Id),
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormState<Id> {
    pub fields: Vec<FormField<Id>>,
    pub focused: usize,
    pub status: Option<FormStatus>,
    pub busy: bool,
    touched: Vec<Id>,
}

impl<Id: Copy + PartialEq> FormState<Id> {
    pub fn new(fields: Vec<FormField<Id>>) -> Self {
        Self {
            fields,
            focused: 0,
            status: None,
            busy: false,
            touched: Vec::new(),
        }
    }

    pub fn field(&self, id: Id) -> Option<&FormField<Id>> {
        self.fields.iter().find(|field| field.id == id)
    }

    pub fn field_mut(&mut self, id: Id) -> Option<&mut FormField<Id>> {
        self.fields.iter_mut().find(|field| field.id == id)
    }

    pub fn focused_id(&self) -> Option<Id> {
        self.fields.get(self.focused).map(|field| field.id)
    }

    pub fn focused_field_mut(&mut self) -> Option<&mut FormField<Id>> {
        self.fields.get_mut(self.focused)
    }

    pub fn is_touched(&self, id: Id) -> bool {
        self.touched.contains(&id)
    }

    pub fn mark_touched(&mut self, id: Id) {
        if !self.touched.contains(&id) {
            self.touched.push(id);
        }
    }

    pub fn text(&self, id: Id) -> String {
        match self.field(id).map(|field| &field.value) {
            Some(FieldValue::Text(input) | FieldValue::Secret(input)) => {
                input.text().trim().to_owned()
            }
            _ => String::new(),
        }
    }

    pub fn optional(&self, id: Id) -> Option<String> {
        Some(self.text(id)).filter(|value| !value.is_empty())
    }

    pub fn choice(&self, id: Id) -> usize {
        match self.field(id).map(|field| &field.value) {
            Some(FieldValue::Choice { selected, .. }) => *selected,
            _ => 0,
        }
    }

    pub fn toggle(&self, id: Id) -> bool {
        matches!(
            self.field(id).map(|field| &field.value),
            Some(FieldValue::Toggle(true))
        )
    }

    pub fn set_text(&mut self, id: Id, value: &str) {
        if let Some(FormField {
            value: FieldValue::Text(input) | FieldValue::Secret(input),
            ..
        }) = self.field_mut(id)
        {
            input.set(value);
        }
    }

    pub fn set_choice(&mut self, id: Id, selected: usize) {
        if let Some(FormField {
            value:
                FieldValue::Choice {
                    options,
                    selected: current,
                },
            ..
        }) = self.field_mut(id)
        {
            *current = selected.min(options.len().saturating_sub(1));
        }
    }

    pub fn move_focus(&mut self, delta: isize, visible: impl Fn(Id) -> bool) {
        let indexes: Vec<usize> = self
            .fields
            .iter()
            .enumerate()
            .filter(|(_, field)| visible(field.id))
            .map(|(index, _)| index)
            .collect();
        let position = indexes
            .iter()
            .position(|index| *index == self.focused)
            .unwrap_or(0);
        let count = indexes.len() as isize;
        let next = (position as isize + delta).rem_euclid(count.max(1)) as usize;
        self.focused = indexes.get(next).copied().unwrap_or(0);
    }

    /// Applies one key to the form: navigation between visible fields, editing of the
    /// focused field (text, cycling choices, flipping toggles) and the shared shortcuts
    /// (Esc cancel, Enter/Ctrl+S submit, Ctrl+T test, other Ctrl+letters as shortcuts).
    /// Returns what happened so the owning dialog can react to edits or commands.
    pub fn handle_key(&mut self, chord: KeyChord, visible: impl Fn(Id) -> bool) -> FormCommand<Id> {
        let ctrl = chord.modifiers.ctrl;
        match chord.code {
            KeyCode::Esc => return FormCommand::Cancel,
            KeyCode::Enter => return FormCommand::Submit,
            KeyCode::Char('s') if ctrl => return FormCommand::Submit,
            KeyCode::Char('t') if ctrl => return FormCommand::Test,
            KeyCode::Char(letter) if ctrl && !matches!(letter, 'w' | 'u' | 'a' | 'e') => {
                return FormCommand::Shortcut(letter);
            }
            KeyCode::Tab | KeyCode::Down => {
                self.move_focus(1, visible);
                return FormCommand::Ignored;
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.move_focus(-1, visible);
                return FormCommand::Ignored;
            }
            _ => {}
        }
        let Some(field) = self.fields.get_mut(self.focused) else {
            return FormCommand::Ignored;
        };
        let id = field.id;
        let changed = match &mut field.value {
            FieldValue::Choice { options, selected } if !options.is_empty() => {
                let delta = match chord.code {
                    KeyCode::Left => -1,
                    KeyCode::Right | KeyCode::Char(' ') => 1,
                    _ => 0,
                };
                if delta != 0 {
                    let count = options.len() as isize;
                    *selected = (*selected as isize + delta).rem_euclid(count) as usize;
                }
                delta != 0
            }
            FieldValue::Choice { .. } => false,
            FieldValue::Toggle(value) => {
                let flip = matches!(
                    chord.code,
                    KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right
                );
                if flip {
                    *value = !*value;
                }
                flip
            }
            FieldValue::Text(input) | FieldValue::Secret(input) => edit_line(input, chord),
        };
        if changed {
            self.mark_touched(id);
            FormCommand::Edited(id)
        } else {
            FormCommand::Ignored
        }
    }

    pub fn paste(&mut self, text: &str) -> Option<Id> {
        let field = self.fields.get_mut(self.focused)?;
        let (FieldValue::Text(input) | FieldValue::Secret(input)) = &mut field.value else {
            return None;
        };
        input.insert_str(text);
        let id = field.id;
        self.mark_touched(id);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_config::Modifiers;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Field {
        Name,
        Hidden,
        Kind,
        Enabled,
    }

    fn key(code: KeyCode) -> KeyChord {
        KeyChord::new(code, Modifiers::NONE)
    }

    fn form() -> FormState<Field> {
        FormState::new(vec![
            FormField::text(Field::Name, "Name", "db"),
            FormField::text(Field::Hidden, "Hidden", ""),
            FormField::choice(Field::Kind, "Kind", vec!["a".into(), "b".into()], 0),
            FormField::toggle(Field::Enabled, "Enabled", false),
        ])
    }

    #[test]
    fn focus_skips_hidden_fields_and_wraps() {
        let mut form = form();
        let visible = |id: Field| id != Field::Hidden;
        form.handle_key(key(KeyCode::Tab), visible);
        assert_eq!(form.focused_id(), Some(Field::Kind));
        form.handle_key(key(KeyCode::Tab), visible);
        form.handle_key(key(KeyCode::Tab), visible);
        assert_eq!(form.focused_id(), Some(Field::Name));
    }

    #[test]
    fn edits_choices_toggles_and_text() {
        let mut form = form();
        assert_eq!(
            form.handle_key(key(KeyCode::Char('x')), |_| true),
            FormCommand::Edited(Field::Name)
        );
        assert_eq!(form.text(Field::Name), "dbx");
        form.focused = 2;
        form.handle_key(key(KeyCode::Right), |_| true);
        assert_eq!(form.choice(Field::Kind), 1);
        form.focused = 3;
        form.handle_key(key(KeyCode::Char(' ')), |_| true);
        assert!(form.toggle(Field::Enabled));
        assert!(form.is_touched(Field::Enabled));
        assert!(!form.is_touched(Field::Hidden));
    }

    #[test]
    fn commands_are_reported() {
        let mut form = form();
        let ctrl = |letter| {
            KeyChord::new(
                KeyCode::Char(letter),
                Modifiers {
                    ctrl: true,
                    ..Modifiers::NONE
                },
            )
        };
        assert_eq!(
            form.handle_key(key(KeyCode::Esc), |_| true),
            FormCommand::Cancel
        );
        assert_eq!(form.handle_key(ctrl('t'), |_| true), FormCommand::Test);
        assert_eq!(
            form.handle_key(ctrl('k'), |_| true),
            FormCommand::Shortcut('k')
        );
        assert_eq!(
            form.handle_key(ctrl('u'), |_| true),
            FormCommand::Edited(Field::Name)
        );
        assert_eq!(form.text(Field::Name), "");
    }
}
