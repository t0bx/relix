/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::command::Command;
use crate::datasource_form::DataSourceForm;
use crate::input::LineInput;
use crate::services::InstallOffer;
use crate::workspace::{PaneId, TabId};
use relix_core::keyvalue::KeyName;
use relix_core::{DataSourceId, DriverId, ProjectId};
use relix_editor::TextBuffer;

#[derive(Debug, Clone, PartialEq)]
pub enum PickerValue {
    Command(Command),
    Project(ProjectId),
    DataSource(DataSourceId),
    History(usize),
    Theme(String),
    Driver(DriverId),
    Filter(String),
    Export(ExportFormat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerPurpose {
    Palette,
    Projects,
    EditorDataSource,
    History,
    Theme,
    Drivers,
    FilterHistory,
    ExportFormat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PickerItem {
    pub label: String,
    pub detail: String,
    pub hint: String,
    pub value: PickerValue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PickerState {
    pub title: String,
    pub purpose: PickerPurpose,
    pub input: LineInput,
    pub items: Vec<PickerItem>,
    pub filtered: Vec<usize>,
    pub selected: usize,
    pub footer: String,
}

impl PickerState {
    pub fn new(title: &str, purpose: PickerPurpose, items: Vec<PickerItem>, footer: &str) -> Self {
        let mut picker = Self {
            title: title.to_owned(),
            purpose,
            input: LineInput::default(),
            filtered: Vec::new(),
            items,
            selected: 0,
            footer: footer.to_owned(),
        };
        picker.refilter();
        picker
    }

    /// Fuzzy-filters items by the typed text (in-order subsequence over label and
    /// detail), ranking prefix and substring matches first.
    pub fn refilter(&mut self) {
        let needle = self.input.text().to_lowercase();
        let mut scored: Vec<(u32, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let haystack = format!("{} {}", item.label, item.detail).to_lowercase();
                relix_editor::completion::match_score(&needle, &haystack)
                    .or_else(|| {
                        relix_editor::completion::match_score(&needle, &item.label.to_lowercase())
                    })
                    .map(|score| (score, index))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        self.filtered = scored.into_iter().map(|(_, index)| index).collect();
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
    }

    pub fn selected_item(&self) -> Option<&PickerItem> {
        self.filtered
            .get(self.selected)
            .and_then(|index| self.items.get(*index))
    }

    pub fn move_selection(&mut self, delta: isize) {
        if !self.filtered.is_empty() {
            let count = self.filtered.len() as isize;
            self.selected = (self.selected as isize + delta).rem_euclid(count) as usize;
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendingAction {
    DeleteProject(ProjectId),
    DeleteDataSource(DataSourceId),
    CloseTab(TabId),
    ClosePane(PaneId),
    CommitChanges(TabId),
    RunStatements {
        tab: TabId,
        script: bool,
    },
    DeleteKey {
        tab: TabId,
        key: KeyName,
    },
    DeleteElement {
        tab: TabId,
    },
    InstallDriver {
        offer: InstallOffer,
        retry: Option<DataSourceId>,
    },
    RemoveDriver(DriverId),
    SwitchProject(ProjectId),
    Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmState {
    pub title: String,
    pub lines: Vec<String>,
    pub confirm_label: String,
    pub danger: bool,
    pub action: PendingAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PromptAction {
    NewProject,
    RenameProject(ProjectId),
    SaveScriptAs(TabId),
    OpenFile,
    ExportResults { tab: TabId, format: ExportFormat },
    ExportProject,
    ImportProject,
    SetTtl { tab: TabId, key: KeyName },
    NewKey { tab: TabId },
    AddElement { tab: TabId },
    Password { source: DataSourceId },
    ResultFilter { tab: TabId },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PromptState {
    pub title: String,
    pub hint: String,
    pub input: LineInput,
    pub masked: bool,
    pub action: PromptAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerState {
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: usize,
    pub copy_text: String,
}

impl ViewerState {
    pub fn new(title: &str, text: &str) -> Self {
        Self {
            title: title.to_owned(),
            lines: text.lines().map(str::to_owned).collect(),
            scroll: 0,
            copy_text: text.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextEditAction {
    RedisString {
        tab: TabId,
        key: KeyName,
    },
    RedisElement {
        tab: TabId,
    },
    GridCell {
        tab: TabId,
        row: usize,
        column: usize,
    },
}

#[derive(Debug, Clone)]
pub struct TextEditState {
    pub title: String,
    pub buffer: TextBuffer,
    pub scroll: usize,
    pub action: TextEditAction,
}

impl TextEditState {
    pub fn new(title: String, text: &str, action: TextEditAction) -> Self {
        let mut buffer = TextBuffer::from_text(text);
        buffer.move_document_end(false);
        Self {
            title,
            buffer,
            scroll: 0,
            action,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortEditorState {
    pub tab: TabId,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnChooserState {
    pub tab: TabId,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JumpTarget {
    Sidebar,
    Pane(PaneId),
    Tab(TabId),
    Row(usize),
    Column(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpState {
    pub targets: Vec<(String, JumpTarget)>,
    pub typed: String,
}

#[derive(Debug, Clone)]
pub enum Overlay {
    Picker(PickerState),
    DataSourceForm(Box<DataSourceForm>),
    Confirm(ConfirmState),
    Prompt(PromptState),
    Viewer(ViewerState),
    TextEdit(Box<TextEditState>),
    SortEditor(SortEditorState),
    ColumnChooser(ColumnChooserState),
    Jump(JumpState),
    Help { scroll: usize },
}

/// Short labels for flash-jump targets: single home-row letters first, then pairs.
pub fn jump_labels(count: usize) -> Vec<String> {
    const KEYS: &[char] = &[
        'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o',
        'p',
    ];
    if count <= KEYS.len() {
        return KEYS.iter().take(count).map(char::to_string).collect();
    }
    KEYS.iter()
        .flat_map(|first| KEYS.iter().map(move |second| format!("{first}{second}")))
        .take(count)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str) -> PickerItem {
        PickerItem {
            label: label.into(),
            detail: String::new(),
            hint: String::new(),
            value: PickerValue::Filter(label.into()),
        }
    }

    #[test]
    fn picker_filters_fuzzily_and_wraps_selection() {
        let mut picker = PickerState::new(
            "t",
            PickerPurpose::Palette,
            vec![item("Split pane"), item("Save script"), item("Quit")],
            "",
        );
        picker.input.set("sp");
        picker.refilter();
        assert_eq!(picker.selected_item().unwrap().label, "Split pane");
        assert_eq!(picker.filtered.len(), 2);
        picker.move_selection(-1);
        assert_eq!(picker.selected_item().unwrap().label, "Save script");
    }

    #[test]
    fn jump_labels_are_unique() {
        let labels = jump_labels(40);
        assert_eq!(labels.len(), 40);
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), 40);
        assert_eq!(jump_labels(2), vec!["a", "s"]);
    }
}
