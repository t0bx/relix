/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::grid::GridState;
use crate::input::LineInput;
use crate::requests::RequestId;
use crate::result_query::ResultQuery;
use relix_core::DataSourceId;
use relix_driver_api::DriverError;
use relix_editor::{CompletionItem, Diagnostic, Position, SearchState, TextBuffer};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorFocus {
    #[default]
    Text,
    Results,
}

#[derive(Debug, Clone)]
pub struct CompletionPopup {
    pub items: Vec<CompletionItem>,
    pub selected: usize,
    pub replace_start: Position,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchField {
    Query,
    Replacement,
}

#[derive(Debug, Clone)]
pub struct SearchBar {
    pub state: SearchState,
    pub query: LineInput,
    pub replacement: LineInput,
    pub replace_mode: bool,
    pub field: SearchField,
}

#[derive(Debug, Clone)]
pub enum ResultKind {
    Rows {
        grid: Box<GridState>,
        query: ResultQuery,
        wrappable: bool,
        loading: bool,
    },
    Affected(u64),
    Message(String),
    Error(DriverError),
}

#[derive(Debug, Clone)]
pub struct ResultEntry {
    pub statement: String,
    pub statement_offset: usize,
    pub kind: ResultKind,
    pub elapsed: Duration,
    pub request: Option<RequestId>,
}

impl ResultEntry {
    pub fn summary(&self) -> String {
        match &self.kind {
            ResultKind::Rows { grid, query, .. } => {
                let more = if query.has_more { "+" } else { "" };
                format!("{}{more} rows", grid.rows.len())
            }
            ResultKind::Affected(rows) => format!("{rows} affected"),
            ResultKind::Message(message) => message.clone(),
            ResultKind::Error(error) => error.kind.label().to_owned(),
        }
    }

    pub fn grid_mut(&mut self) -> Option<&mut GridState> {
        match &mut self.kind {
            ResultKind::Rows { grid, .. } => Some(&mut **grid),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunningQuery {
    pub request: RequestId,
    pub cancel: CancellationToken,
    pub started: Instant,
    pub total: usize,
    pub completed: usize,
    pub statement_offsets: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct EditorTab {
    pub title: String,
    pub path: Option<PathBuf>,
    pub draft_id: String,
    pub data_source: Option<DataSourceId>,
    pub buffer: TextBuffer,
    pub scroll_line: usize,
    pub scroll_column: usize,
    pub completion: Option<CompletionPopup>,
    pub search: Option<SearchBar>,
    pub diagnostics: Vec<Diagnostic>,
    pub results: Vec<ResultEntry>,
    pub active_result: usize,
    pub focus: EditorFocus,
    pub running: Option<RunningQuery>,
    pub in_transaction: bool,
}

impl EditorTab {
    pub fn new(title: String, data_source: Option<DataSourceId>, text: &str) -> Self {
        Self {
            title,
            path: None,
            draft_id: uuid::Uuid::new_v4().simple().to_string(),
            data_source,
            buffer: TextBuffer::from_text(text),
            scroll_line: 0,
            scroll_column: 0,
            completion: None,
            search: None,
            diagnostics: Vec::new(),
            results: Vec::new(),
            active_result: 0,
            focus: EditorFocus::Text,
            running: None,
            in_transaction: false,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.is_modified() || (self.path.is_none() && !self.buffer.text().trim().is_empty())
    }

    pub fn active_result(&self) -> Option<&ResultEntry> {
        self.results.get(self.active_result)
    }

    pub fn active_result_mut(&mut self) -> Option<&mut ResultEntry> {
        self.results.get_mut(self.active_result)
    }

    pub fn active_grid_mut(&mut self) -> Option<&mut GridState> {
        self.active_result_mut().and_then(ResultEntry::grid_mut)
    }

    pub fn ensure_cursor_visible(&mut self, height: usize, width: usize) {
        let cursor = self.buffer.cursor();
        let height = height.max(1);
        let width = width.max(1);
        if cursor.line < self.scroll_line {
            self.scroll_line = cursor.line;
        } else if cursor.line >= self.scroll_line + height {
            self.scroll_line = cursor.line + 1 - height;
        }
        if cursor.column < self.scroll_column {
            self.scroll_column = cursor.column;
        } else if cursor.column >= self.scroll_column + width {
            self.scroll_column = cursor.column + 1 - width;
        }
    }
}
