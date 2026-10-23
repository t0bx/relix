/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::editor_tab::{EditorFocus, EditorTab};
use crate::grid::GridState;
use crate::keyvalue_tab::KeyValueTab;
use crate::table_tab::{TableFocus, TableTab};
use relix_core::DataSourceId;

#[derive(Debug, Clone)]
pub enum Tab {
    Editor(Box<EditorTab>),
    Table(Box<TableTab>),
    KeyValue(Box<KeyValueTab>),
}

impl Tab {
    pub fn title(&self) -> String {
        match self {
            Self::Editor(editor) => {
                let marker = if editor.is_dirty() { "●" } else { "" };
                format!("{}{marker}", editor.title)
            }
            Self::Table(table) => {
                let pending = table
                    .grid
                    .as_ref()
                    .is_some_and(GridState::has_pending_changes);
                format!("{}{}", table.title(), if pending { "*" } else { "" })
            }
            Self::KeyValue(_) => "keys".to_owned(),
        }
    }

    pub fn data_source(&self) -> Option<DataSourceId> {
        match self {
            Self::Editor(editor) => editor.data_source,
            Self::Table(table) => Some(table.data_source),
            Self::KeyValue(tab) => Some(tab.data_source),
        }
    }

    /// The grid the keyboard currently drives inside this tab, if any.
    pub fn focused_grid_mut(&mut self) -> Option<&mut GridState> {
        match self {
            Self::Editor(editor) if editor.focus == EditorFocus::Results => {
                editor.active_grid_mut()
            }
            Self::Table(table) if table.focus == TableFocus::Grid => table.grid.as_mut(),
            _ => None,
        }
    }

    pub fn has_pending_changes(&self) -> bool {
        match self {
            Self::Table(table) => table
                .grid
                .as_ref()
                .is_some_and(GridState::has_pending_changes),
            _ => false,
        }
    }

    pub fn as_editor_mut(&mut self) -> Option<&mut EditorTab> {
        match self {
            Self::Editor(editor) => Some(editor),
            _ => None,
        }
    }

    pub fn as_table_mut(&mut self) -> Option<&mut TableTab> {
        match self {
            Self::Table(table) => Some(table),
            _ => None,
        }
    }

    pub fn as_key_value_mut(&mut self) -> Option<&mut KeyValueTab> {
        match self {
            Self::KeyValue(tab) => Some(tab),
            _ => None,
        }
    }
}
