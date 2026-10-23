/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use crate::command::Command;
use crate::grid::{GridState, GridView};
use crate::notifications::Level;
use crate::overlay::{
    ColumnChooserState, ExportFormat, JumpTarget, Overlay, PickerItem, PickerPurpose, PickerState,
    PickerValue, ViewerState,
};
use crate::tab::Tab;
use crate::workspace::TabId;
use relix_core::Value;

impl App {
    fn focused_grid(&mut self) -> Option<&mut GridState> {
        self.active_tab_mut().and_then(Tab::focused_grid_mut)
    }

    /// Commands every grid understands, for table tabs and query results alike.
    /// Returns `false` when the command is not a generic grid command.
    pub(crate) fn grid_command(&mut self, command: Command) -> bool {
        let Some(grid) = self.focused_grid() else {
            return false;
        };
        let page = grid.viewport_rows.max(1) as isize;
        if grid.view == GridView::Json {
            match command {
                Command::MoveUp => grid.json_scroll = grid.json_scroll.saturating_sub(1),
                Command::MoveDown => grid.json_scroll += 1,
                Command::PageUp => {
                    grid.json_scroll = grid.json_scroll.saturating_sub(page as usize)
                }
                Command::PageDown => grid.json_scroll += page as usize,
                Command::MoveTop => grid.json_scroll = 0,
                Command::ToggleJsonView => grid.view = GridView::Table,
                Command::CopyRows | Command::CopyCell => {
                    let text = grid.json_text();
                    self.copy_to_clipboard(&text);
                }
                _ => return false,
            }
            return true;
        }
        match command {
            Command::MoveUp => grid.move_rows(-1),
            Command::MoveDown => grid.move_rows(1),
            Command::MoveLeft => grid.move_columns(-1),
            Command::MoveRight => grid.move_columns(1),
            Command::PageUp => grid.move_rows(-page),
            Command::PageDown => grid.move_rows(page),
            Command::MoveTop => grid.move_to_row(0),
            Command::MoveBottom => grid.move_to_row(usize::MAX),
            Command::MoveLineStart => grid.first_column(),
            Command::MoveLineEnd => grid.last_column(),
            Command::ToggleRowSelection => grid.toggle_row_selection(),
            Command::ExtendSelectionUp => grid.extend_selection(-1),
            Command::ExtendSelectionDown => grid.extend_selection(1),
            Command::SelectAll => grid.select_all_rows(),
            Command::WidenColumn => grid.resize_column(4),
            Command::NarrowColumn => grid.resize_column(-4),
            Command::HideColumn => {
                if !grid.hide_current_column() {
                    self.status = "at least one column must stay visible".into();
                }
            }
            Command::ToggleJsonView => {
                grid.view = GridView::Json;
                grid.json_scroll = 0;
            }
            Command::CopyCell => {
                if let Some(text) = grid.copy_current_cell() {
                    self.copy_to_clipboard(&text);
                }
            }
            Command::CopyRows => {
                let text = grid.copy_target_rows();
                self.copy_to_clipboard(&text);
            }
            Command::InspectCell => self.inspect_current_cell(),
            Command::JumpRow => {
                let first = grid.row_offset;
                let last = (first + grid.viewport_rows).min(grid.rows.len());
                let targets = (first..last).map(JumpTarget::Row).collect();
                self.start_jump(targets);
            }
            Command::JumpColumn => {
                let targets = grid
                    .visible_columns()
                    .into_iter()
                    .map(JumpTarget::Column)
                    .collect();
                self.start_jump(targets);
            }
            Command::ChooseColumns => {
                if let Some(tab) = self.workspace.active_tab_id() {
                    self.overlays
                        .push(Overlay::ColumnChooser(ColumnChooserState {
                            tab,
                            selected: 0,
                        }));
                }
            }
            Command::ExportResults => self.open_export_picker(),
            _ => return false,
        }
        true
    }

    fn inspect_current_cell(&mut self) {
        let Some(grid) = self.focused_grid() else {
            return;
        };
        let Some(value) = grid.current_value().cloned() else {
            return;
        };
        let column = grid.current_column().map(|column| column.meta.clone());
        let (title, text) = describe_value(
            column
                .as_ref()
                .map_or("value", |column| column.name.as_str()),
            &value,
        );
        let mut viewer = ViewerState::new(&title, &text);
        viewer.copy_text = value.to_plain_string().unwrap_or_default();
        self.overlays.push(Overlay::Viewer(viewer));
    }

    fn open_export_picker(&mut self) {
        let items = [ExportFormat::Csv, ExportFormat::Json]
            .into_iter()
            .map(|format| PickerItem {
                label: format.extension().to_uppercase(),
                detail: match format {
                    ExportFormat::Csv => "comma separated values".into(),
                    ExportFormat::Json => "array of objects".into(),
                },
                hint: String::new(),
                value: PickerValue::Export(format),
            })
            .collect();
        self.overlays.push(Overlay::Picker(PickerState::new(
            "Export loaded rows",
            PickerPurpose::ExportFormat,
            items,
            "Enter choose · Esc cancel",
        )));
    }

    pub(crate) fn export_grid(&mut self, tab: TabId, format: ExportFormat, path: &str) {
        let Some(grid) = self.workspace.tab_mut(tab).and_then(Tab::focused_grid_mut) else {
            self.notify(Level::Error, "the results to export are no longer open");
            return;
        };
        let (columns, rows) = grid.current_values();
        let text = match format {
            ExportFormat::Csv => relix_query::export::to_csv(&columns, &rows),
            ExportFormat::Json => relix_query::export::to_json(&columns, &rows),
        };
        let path = std::path::PathBuf::from(path.trim());
        match std::fs::write(&path, text) {
            Ok(()) => self.notify(
                Level::Success,
                format!("exported {} rows to {}", rows.len(), path.display()),
            ),
            Err(error) => self.notify(Level::Error, format!("export failed: {error}")),
        }
    }
}

/// Builds the inspector text for a cell: JSON documents are pretty-printed, binary data
/// is shown as a hex dump, everything else verbatim.
pub fn describe_value(column: &str, value: &Value) -> (String, String) {
    match value {
        Value::Null => (format!("{column} · NULL"), "NULL".to_owned()),
        Value::Bytes(bytes) => (format!("{column} · {} bytes", bytes.len()), hex_dump(bytes)),
        Value::Text(text) => {
            let pretty = serde_json::from_str::<serde_json::Value>(text)
                .ok()
                .filter(|json| json.is_object() || json.is_array())
                .and_then(|json| serde_json::to_string_pretty(&json).ok());
            match pretty {
                Some(pretty) => (format!("{column} · json"), pretty),
                None => (
                    format!("{column} · {} chars", text.chars().count()),
                    text.clone(),
                ),
            }
        }
        other => (
            column.to_owned(),
            other.to_plain_string().unwrap_or_default(),
        ),
    }
}

fn hex_dump(bytes: &[u8]) -> String {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(row, chunk)| {
            let hex: Vec<String> = chunk.iter().map(|byte| format!("{byte:02x}")).collect();
            let ascii: String = chunk
                .iter()
                .map(|byte| {
                    if byte.is_ascii_graphic() || *byte == b' ' {
                        char::from(*byte)
                    } else {
                        '.'
                    }
                })
                .collect();
            format!("{:08x}  {:<47}  {ascii}", row * 16, hex.join(" "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_text_is_pretty_printed() {
        let (title, text) = describe_value("doc", &Value::Text("{\"a\":1}".into()));
        assert_eq!(title, "doc · json");
        assert!(text.contains("\n  \"a\": 1\n"));
    }

    #[test]
    fn binary_values_get_a_hex_dump() {
        let (title, text) = describe_value("blob", &Value::Bytes(b"hi\x00".to_vec()));
        assert_eq!(title, "blob · 3 bytes");
        assert_eq!(text, format!("00000000  {:<47}  hi.", "68 69 00"));
    }
}
