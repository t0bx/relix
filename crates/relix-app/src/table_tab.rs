/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::grid::{GridState, GridView};
use crate::requests::RequestId;
use crate::result_query::ResultQuery;
use relix_core::{DataSourceId, TableDetails, TableRef};
use relix_editor::CompletionItem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableFocus {
    #[default]
    Grid,
    Filter,
}

#[derive(Debug, Clone)]
pub struct FilterCompletion {
    pub items: Vec<CompletionItem>,
    pub selected: usize,
    pub replace_start: usize,
    pub from_history: bool,
}

#[derive(Debug, Clone)]
pub struct TableTab {
    pub data_source: DataSourceId,
    pub table: TableRef,
    pub details: Option<TableDetails>,
    pub grid: Option<GridState>,
    pub query: ResultQuery,
    pub focus: TableFocus,
    pub completion: Option<FilterCompletion>,
    pub loading: bool,
    pub error: Option<String>,
    pub request: Option<RequestId>,
    pub restore_hidden: Vec<String>,
    pub restore_view: GridView,
}

impl TableTab {
    pub fn new(data_source: DataSourceId, table: TableRef, page_size: u32) -> Self {
        Self {
            data_source,
            table,
            details: None,
            grid: None,
            query: ResultQuery::new(page_size),
            focus: TableFocus::Grid,
            completion: None,
            loading: true,
            error: None,
            request: None,
            restore_hidden: Vec::new(),
            restore_view: GridView::Table,
        }
    }

    /// Creates an independent copy for another pane: same table, filter and sort, but its
    /// own grid, selection and scroll state.
    pub fn duplicate(&self) -> Self {
        let mut copy = Self::new(self.data_source, self.table.clone(), self.query.page_size);
        copy.query.sort = self.query.sort.clone();
        copy.query.filter_input = self.query.filter_input.clone();
        copy.query.applied_filter = self.query.applied_filter.clone();
        copy.details = self.details.clone();
        copy
    }

    pub fn title(&self) -> String {
        self.table.name.clone()
    }

    pub fn filter_key(&self) -> String {
        format!("{}:{}", self.data_source, self.table)
    }
}
