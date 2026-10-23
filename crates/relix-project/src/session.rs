/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{DataSourceId, SortKey, TableRef};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const SESSION_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorSnapshot {
    pub title: String,
    pub script: Option<PathBuf>,
    pub draft: Option<String>,
    pub data_source: Option<DataSourceId>,
    pub cursor_line: usize,
    pub cursor_column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableSnapshot {
    pub data_source: DataSourceId,
    pub table: TableRef,
    pub filter: Option<String>,
    pub sort: Vec<SortKey>,
    pub json_view: bool,
    pub hidden_columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyValueSnapshot {
    pub data_source: DataSourceId,
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TabSnapshot {
    Editor(EditorSnapshot),
    Table(TableSnapshot),
    KeyValue(KeyValueSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub tabs: Vec<TabSnapshot>,
    pub active: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "node", rename_all = "snake_case")]
pub enum LayoutSnapshot {
    Pane(PaneSnapshot),
    Split {
        direction: SplitDirection,
        ratio: u16,
        first: Box<LayoutSnapshot>,
        second: Box<LayoutSnapshot>,
    },
}

impl Default for LayoutSnapshot {
    fn default() -> Self {
        Self::Pane(PaneSnapshot::default())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    pub version: u32,
    pub layout: LayoutSnapshot,
    pub focused_pane: usize,
    pub sidebar_visible: bool,
    pub sidebar_width: Option<u16>,
    pub connected: Vec<DataSourceId>,
    pub expanded_nodes: Vec<String>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            version: SESSION_FORMAT_VERSION,
            layout: LayoutSnapshot::default(),
            focused_pane: 0,
            sidebar_visible: true,
            sidebar_width: None,
            connected: Vec::new(),
            expanded_nodes: Vec::new(),
        }
    }
}
