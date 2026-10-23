/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::input::LineInput;
use relix_core::value::truncate_chars;
use relix_core::{
    CellChange, ChangeSet, ColumnMeta, RowBatch, RowChange, SortDirection, SortKey, TableRef, Value,
};
use std::collections::{BTreeMap, BTreeSet};

const MIN_COLUMN_WIDTH: u16 = 4;
const MAX_COLUMN_WIDTH: u16 = 60;
const DEFAULT_MAX_WIDTH: u16 = 32;
const WIDTH_SAMPLE_ROWS: usize = 200;
const SORT_MARKER_WIDTH: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct GridColumn {
    pub meta: ColumnMeta,
    pub width: u16,
    pub hidden: bool,
    pub is_key: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowOrigin {
    Fetched,
    Inserted,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GridRow {
    pub values: Vec<Value>,
    pub edits: BTreeMap<usize, Value>,
    pub origin: RowOrigin,
    pub deleted: bool,
}

impl GridRow {
    pub fn value(&self, column: usize) -> &Value {
        self.edits
            .get(&column)
            .or_else(|| self.values.get(column))
            .unwrap_or(&Value::Null)
    }

    pub fn is_modified(&self, column: usize) -> bool {
        self.edits.contains_key(&column)
    }

    pub fn has_changes(&self) -> bool {
        self.deleted || self.origin == RowOrigin::Inserted || !self.edits.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridView {
    #[default]
    Table,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editability {
    Editable { key_columns: Vec<String> },
    ReadOnly(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellEditor {
    pub row: usize,
    pub column: usize,
    pub input: LineInput,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GridState {
    pub columns: Vec<GridColumn>,
    pub rows: Vec<GridRow>,
    pub cursor_row: usize,
    pub cursor_column: usize,
    pub row_offset: usize,
    pub column_offset: usize,
    pub selected: BTreeSet<usize>,
    anchor: Option<usize>,
    pub view: GridView,
    pub json_scroll: usize,
    pub editing: Option<CellEditor>,
    pub editability: Editability,
    pub viewport_rows: usize,
}

impl GridState {
    pub fn from_batch(batch: &RowBatch, editability: Editability) -> Self {
        let key_columns = match &editability {
            Editability::Editable { key_columns } => key_columns.clone(),
            Editability::ReadOnly(_) => Vec::new(),
        };
        let columns = batch
            .columns
            .iter()
            .enumerate()
            .map(|(index, meta)| GridColumn {
                width: measure_width(meta, &batch.rows, index),
                meta: meta.clone(),
                hidden: false,
                is_key: key_columns.contains(&meta.name),
            })
            .collect();
        Self {
            columns,
            rows: fetched_rows(batch),
            cursor_row: 0,
            cursor_column: 0,
            row_offset: 0,
            column_offset: 0,
            selected: BTreeSet::new(),
            anchor: None,
            view: GridView::Table,
            json_scroll: 0,
            editing: None,
            editability,
            viewport_rows: 20,
        }
    }

    /// Replaces the rows with a new page while keeping column layout (widths, hidden
    /// columns) when the column set is unchanged, e.g. after paging or re-sorting.
    pub fn replace_rows(&mut self, batch: &RowBatch) {
        let same_columns = self.columns.len() == batch.columns.len()
            && self
                .columns
                .iter()
                .zip(&batch.columns)
                .all(|(column, meta)| column.meta.name == meta.name);
        if !same_columns {
            let editability = self.editability.clone();
            let view = self.view;
            *self = Self::from_batch(batch, editability);
            self.view = view;
            return;
        }
        self.rows = fetched_rows(batch);
        self.cursor_row = self.cursor_row.min(self.rows.len().saturating_sub(1));
        self.row_offset = 0;
        self.selected.clear();
        self.anchor = None;
        self.editing = None;
        self.json_scroll = 0;
    }

    pub fn is_read_only(&self) -> bool {
        matches!(self.editability, Editability::ReadOnly(_))
    }

    pub fn current_value(&self) -> Option<&Value> {
        self.rows
            .get(self.cursor_row)
            .map(|row| row.value(self.cursor_column))
    }

    pub fn current_column(&self) -> Option<&GridColumn> {
        self.columns.get(self.cursor_column)
    }

    pub fn visible_columns(&self) -> Vec<usize> {
        self.columns
            .iter()
            .enumerate()
            .filter(|(_, column)| !column.hidden)
            .map(|(index, _)| index)
            .collect()
    }

    pub fn move_rows(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        self.cursor_row = self.cursor_row.saturating_add_signed(delta).min(last);
        self.anchor = None;
        self.ensure_row_visible();
    }

    pub fn move_to_row(&mut self, row: usize) {
        self.cursor_row = row.min(self.rows.len().saturating_sub(1));
        self.ensure_row_visible();
    }

    pub fn move_columns(&mut self, delta: isize) {
        let visible = self.visible_columns();
        let Some(position) = visible
            .iter()
            .position(|index| *index == self.cursor_column)
        else {
            self.cursor_column = visible.first().copied().unwrap_or(0);
            return;
        };
        let target = position
            .saturating_add_signed(delta)
            .min(visible.len().saturating_sub(1));
        self.cursor_column = visible[target];
    }

    pub fn move_to_column(&mut self, column: usize) {
        if self
            .columns
            .get(column)
            .is_some_and(|column| !column.hidden)
        {
            self.cursor_column = column;
        }
    }

    pub fn first_column(&mut self) {
        if let Some(first) = self.visible_columns().first() {
            self.cursor_column = *first;
        }
    }

    pub fn last_column(&mut self) {
        if let Some(last) = self.visible_columns().last() {
            self.cursor_column = *last;
        }
    }

    pub fn ensure_row_visible(&mut self) {
        let height = self.viewport_rows.max(1);
        if self.cursor_row < self.row_offset {
            self.row_offset = self.cursor_row;
        } else if self.cursor_row >= self.row_offset + height {
            self.row_offset = self.cursor_row + 1 - height;
        }
    }

    pub fn toggle_row_selection(&mut self) {
        if !self.selected.remove(&self.cursor_row) {
            self.selected.insert(self.cursor_row);
        }
        self.anchor = Some(self.cursor_row);
    }

    pub fn extend_selection(&mut self, delta: isize) {
        let anchor = *self.anchor.get_or_insert(self.cursor_row);
        self.cursor_row = self
            .cursor_row
            .saturating_add_signed(delta)
            .min(self.rows.len().saturating_sub(1));
        let (from, to) = (anchor.min(self.cursor_row), anchor.max(self.cursor_row));
        self.selected = (from..=to).collect();
        self.ensure_row_visible();
    }

    pub fn select_all_rows(&mut self) {
        self.selected = (0..self.rows.len()).collect();
    }

    /// The rows an action applies to: the explicit selection, or the cursor row.
    pub fn target_rows(&self) -> Vec<usize> {
        if self.selected.is_empty() {
            (self.cursor_row < self.rows.len())
                .then_some(vec![self.cursor_row])
                .unwrap_or_default()
        } else {
            self.selected.iter().copied().collect()
        }
    }

    pub fn resize_column(&mut self, delta: i16) {
        if let Some(column) = self.columns.get_mut(self.cursor_column) {
            let width = i32::from(column.width) + i32::from(delta);
            column.width =
                width.clamp(i32::from(MIN_COLUMN_WIDTH), i32::from(MAX_COLUMN_WIDTH) * 4) as u16;
        }
    }

    pub fn hide_current_column(&mut self) -> bool {
        if self.visible_columns().len() <= 1 {
            return false;
        }
        let hidden = self.cursor_column;
        if let Some(column) = self.columns.get_mut(hidden) {
            column.hidden = true;
        }
        let visible = self.visible_columns();
        self.cursor_column = visible
            .iter()
            .copied()
            .find(|index| *index > hidden)
            .or_else(|| visible.iter().copied().rev().find(|index| *index < hidden))
            .unwrap_or(0);
        true
    }

    pub fn set_column_hidden(&mut self, column: usize, hidden: bool) {
        let visible = self.visible_columns().len();
        if let Some(target) = self.columns.get_mut(column)
            && (!hidden || visible > 1)
        {
            target.hidden = hidden;
        }
        if self
            .columns
            .get(self.cursor_column)
            .is_some_and(|column| column.hidden)
        {
            self.first_column();
        }
    }

    pub fn hidden_column_names(&self) -> Vec<String> {
        self.columns
            .iter()
            .filter(|column| column.hidden)
            .map(|column| column.meta.name.clone())
            .collect()
    }

    pub fn hide_columns_named(&mut self, names: &[String]) {
        for column in &mut self.columns {
            column.hidden = names.contains(&column.meta.name);
        }
        if self.visible_columns().is_empty()
            && let Some(first) = self.columns.first_mut()
        {
            first.hidden = false;
        }
    }

    pub fn begin_edit(&mut self) -> Result<(), String> {
        if let Editability::ReadOnly(reason) = &self.editability {
            return Err(reason.clone());
        }
        let row = self.rows.get(self.cursor_row).ok_or("no row selected")?;
        if row.deleted {
            return Err("row is marked for deletion".into());
        }
        let text = row
            .value(self.cursor_column)
            .to_plain_string()
            .unwrap_or_default();
        self.editing = Some(CellEditor {
            row: self.cursor_row,
            column: self.cursor_column,
            input: LineInput::with_text(&text),
        });
        Ok(())
    }

    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }

    /// Stores the edited text as a pending change. Values are kept as text and coerced
    /// by the database when the change set is committed; nothing is written here.
    pub fn commit_edit(&mut self) {
        let Some(editor) = self.editing.take() else {
            return;
        };
        self.stage_value(
            editor.row,
            editor.column,
            Value::Text(editor.input.text().to_owned()),
        );
    }

    pub fn set_cell(&mut self, row: usize, column: usize, value: Value) -> Result<(), String> {
        if let Editability::ReadOnly(reason) = &self.editability {
            return Err(reason.clone());
        }
        if self.rows.get(row).is_none_or(|row| row.deleted) {
            return Err("row is no longer editable".into());
        }
        self.stage_value(row, column, value);
        Ok(())
    }

    pub fn set_null(&mut self) -> Result<(), String> {
        if let Editability::ReadOnly(reason) = &self.editability {
            return Err(reason.clone());
        }
        for row in self.target_rows() {
            self.stage_value(row, self.cursor_column, Value::Null);
        }
        Ok(())
    }

    fn stage_value(&mut self, row_index: usize, column: usize, value: Value) {
        let Some(row) = self.rows.get_mut(row_index) else {
            return;
        };
        let unchanged = row.origin == RowOrigin::Fetched
            && row
                .values
                .get(column)
                .is_some_and(|original| *original == value);
        if unchanged {
            row.edits.remove(&column);
        } else {
            row.edits.insert(column, value);
        }
    }

    pub fn insert_row(&mut self) -> Result<(), String> {
        if let Editability::ReadOnly(reason) = &self.editability {
            return Err(reason.clone());
        }
        self.rows.push(GridRow {
            values: vec![Value::Null; self.columns.len()],
            edits: BTreeMap::new(),
            origin: RowOrigin::Inserted,
            deleted: false,
        });
        self.cursor_row = self.rows.len() - 1;
        self.ensure_row_visible();
        Ok(())
    }

    pub fn delete_target_rows(&mut self) -> Result<usize, String> {
        if let Editability::ReadOnly(reason) = &self.editability {
            return Err(reason.clone());
        }
        let targets = self.target_rows();
        let count = targets.len();
        for index in targets.iter().rev() {
            match self.rows.get(*index).map(|row| row.origin) {
                Some(RowOrigin::Inserted) => {
                    self.rows.remove(*index);
                }
                Some(RowOrigin::Fetched) => {
                    if let Some(row) = self.rows.get_mut(*index) {
                        row.deleted = true;
                    }
                }
                None => {}
            }
        }
        self.selected.clear();
        self.cursor_row = self.cursor_row.min(self.rows.len().saturating_sub(1));
        Ok(count)
    }

    pub fn revert_current(&mut self) {
        let column = self.cursor_column;
        let Some(row) = self.rows.get_mut(self.cursor_row) else {
            return;
        };
        if row.origin == RowOrigin::Inserted {
            self.rows.remove(self.cursor_row);
            self.cursor_row = self.cursor_row.min(self.rows.len().saturating_sub(1));
        } else if row.deleted {
            row.deleted = false;
        } else if row.edits.remove(&column).is_none() {
            row.edits.clear();
        }
    }

    pub fn discard_changes(&mut self) {
        self.rows.retain(|row| row.origin == RowOrigin::Fetched);
        for row in &mut self.rows {
            row.edits.clear();
            row.deleted = false;
        }
        self.cursor_row = self.cursor_row.min(self.rows.len().saturating_sub(1));
        self.editing = None;
    }

    pub fn pending_count(&self) -> usize {
        self.rows.iter().filter(|row| row.has_changes()).count()
    }

    pub fn has_pending_changes(&self) -> bool {
        self.rows.iter().any(GridRow::has_changes)
    }

    /// Translates pending grid edits into a database change set. Updates and deletes are
    /// keyed by the row identity columns using the values originally fetched, and updates
    /// carry each cell's original value so the driver can detect concurrent changes.
    pub fn build_change_set(&self, table: &TableRef) -> Result<ChangeSet, String> {
        let key_columns = match &self.editability {
            Editability::Editable { key_columns } => key_columns,
            Editability::ReadOnly(reason) => return Err(reason.clone()),
        };
        let key_indexes: Vec<(String, usize)> = key_columns
            .iter()
            .map(|name| {
                self.column_index(name)
                    .map(|index| (name.clone(), index))
                    .ok_or_else(|| format!("key column '{name}' is not part of the result"))
            })
            .collect::<Result<_, _>>()?;
        let mut changes = Vec::new();
        for row in &self.rows {
            let key = || -> Vec<(String, Value)> {
                key_indexes
                    .iter()
                    .map(|(name, index)| (name.clone(), row.values[*index].clone()))
                    .collect()
            };
            match (row.origin, row.deleted) {
                (RowOrigin::Fetched, true) => changes.push(RowChange::Delete { key: key() }),
                (RowOrigin::Fetched, false) if !row.edits.is_empty() => {
                    let cells = row
                        .edits
                        .iter()
                        .map(|(column, updated)| CellChange {
                            column: self.columns[*column].meta.name.clone(),
                            original: row.values[*column].clone(),
                            updated: updated.clone(),
                        })
                        .collect();
                    changes.push(RowChange::Update { key: key(), cells });
                }
                (RowOrigin::Inserted, _) => {
                    let values = row
                        .edits
                        .iter()
                        .map(|(column, value)| {
                            (self.columns[*column].meta.name.clone(), value.clone())
                        })
                        .collect();
                    changes.push(RowChange::Insert { values });
                }
                _ => {}
            }
        }
        Ok(ChangeSet {
            table: table.clone(),
            changes,
        })
    }

    pub fn change_summary(&self) -> Vec<String> {
        let describe_key = |row: &GridRow| -> String {
            self.columns
                .iter()
                .enumerate()
                .filter(|(_, column)| column.is_key)
                .map(|(index, column)| {
                    format!(
                        "{}={}",
                        column.meta.name,
                        row.values[index].display_preview(24)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut lines = Vec::new();
        for row in &self.rows {
            match (row.origin, row.deleted) {
                (RowOrigin::Fetched, true) => lines.push(format!("DELETE  {}", describe_key(row))),
                (RowOrigin::Fetched, false) => {
                    for (column, updated) in &row.edits {
                        lines.push(format!(
                            "UPDATE  {}  {}: {} → {}",
                            describe_key(row),
                            self.columns[*column].meta.name,
                            row.values[*column].display_preview(24),
                            updated.display_preview(24)
                        ));
                    }
                }
                (RowOrigin::Inserted, _) => {
                    let values: Vec<String> = row
                        .edits
                        .iter()
                        .map(|(column, value)| {
                            format!(
                                "{}={}",
                                self.columns[*column].meta.name,
                                value.display_preview(16)
                            )
                        })
                        .collect();
                    lines.push(format!("INSERT  {}", values.join(", ")));
                }
            }
        }
        lines
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|column| column.meta.name == name)
    }

    pub fn copy_current_cell(&self) -> Option<String> {
        self.current_value()
            .map(|value| value.to_plain_string().unwrap_or_else(|| "NULL".to_owned()))
    }

    pub fn copy_target_rows(&self) -> String {
        let visible = self.visible_columns();
        let mut lines = vec![
            visible
                .iter()
                .map(|index| self.columns[*index].meta.name.clone())
                .collect::<Vec<_>>()
                .join("\t"),
        ];
        for row_index in self.target_rows() {
            let row = &self.rows[row_index];
            lines.push(
                visible
                    .iter()
                    .map(|index| {
                        row.value(*index)
                            .to_plain_string()
                            .unwrap_or_default()
                            .replace(['\t', '\n'], " ")
                    })
                    .collect::<Vec<_>>()
                    .join("\t"),
            );
        }
        lines.join("\n")
    }

    pub fn current_values(&self) -> (Vec<ColumnMeta>, Vec<Vec<Value>>) {
        let visible = self.visible_columns();
        let columns = visible
            .iter()
            .map(|index| self.columns[*index].meta.clone())
            .collect();
        let rows = self
            .rows
            .iter()
            .filter(|row| !row.deleted)
            .map(|row| {
                visible
                    .iter()
                    .map(|index| row.value(*index).clone())
                    .collect()
            })
            .collect();
        (columns, rows)
    }

    /// Sorts the loaded rows in memory, used when a result cannot be re-queried.
    pub fn sort_locally(&mut self, keys: &[SortKey]) {
        let indexes: Vec<(usize, SortDirection)> = keys
            .iter()
            .filter_map(|key| {
                self.column_index(&key.column)
                    .map(|index| (index, key.direction))
            })
            .collect();
        self.rows.sort_by(|left, right| {
            for (column, direction) in &indexes {
                let ordering = relix_query::ordering::compare_values(
                    left.value(*column),
                    right.value(*column),
                );
                let ordering = match direction {
                    SortDirection::Ascending => ordering,
                    SortDirection::Descending => ordering.reverse(),
                };
                if ordering != std::cmp::Ordering::Equal {
                    return ordering;
                }
            }
            std::cmp::Ordering::Equal
        });
        self.selected.clear();
    }

    pub fn json_text(&self) -> String {
        let (columns, rows) = self.current_values();
        relix_query::export::to_json(&columns, &rows)
    }
}

fn fetched_rows(batch: &RowBatch) -> Vec<GridRow> {
    batch
        .rows
        .iter()
        .map(|values| GridRow {
            values: values.clone(),
            edits: BTreeMap::new(),
            origin: RowOrigin::Fetched,
            deleted: false,
        })
        .collect()
}

fn measure_width(meta: &ColumnMeta, rows: &[Vec<Value>], index: usize) -> u16 {
    let header = meta.name.chars().count() + SORT_MARKER_WIDTH;
    let content = rows
        .iter()
        .take(WIDTH_SAMPLE_ROWS)
        .filter_map(|row| row.get(index))
        .map(|value| {
            value
                .display_preview(usize::from(DEFAULT_MAX_WIDTH))
                .chars()
                .count()
        })
        .max()
        .unwrap_or(0);
    let width = header.max(content).max(usize::from(MIN_COLUMN_WIDTH));
    u16::try_from(width)
        .unwrap_or(DEFAULT_MAX_WIDTH)
        .min(DEFAULT_MAX_WIDTH)
}

pub fn cell_text(value: &Value, width: usize) -> String {
    truncate_chars(&value.display_preview(width.max(1)), width.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn batch() -> RowBatch {
        RowBatch {
            columns: vec![
                ColumnMeta::new("id", Some("int".into())),
                ColumnMeta::new("name", Some("text".into())),
                ColumnMeta::new("note", None),
            ],
            rows: vec![
                vec![Value::Int(1), Value::Text("ada".into()), Value::Null],
                vec![
                    Value::Int(2),
                    Value::Text("bob".into()),
                    Value::Text("x".into()),
                ],
                vec![Value::Int(3), Value::Text("cy".into()), Value::Null],
            ],
            has_more: false,
        }
    }

    fn editable() -> GridState {
        GridState::from_batch(
            &batch(),
            Editability::Editable {
                key_columns: vec!["id".into()],
            },
        )
    }

    fn table() -> TableRef {
        TableRef::new(None, Some("public".into()), "users")
    }

    #[test]
    fn navigation_and_selection() {
        let mut grid = editable();
        grid.viewport_rows = 2;
        grid.move_rows(5);
        assert_eq!(grid.cursor_row, 2);
        assert_eq!(grid.row_offset, 1);
        grid.move_columns(1);
        grid.hide_current_column();
        assert_eq!(grid.cursor_column, 2);
        grid.move_columns(-1);
        assert_eq!(grid.cursor_column, 0);
        grid.extend_selection(-2);
        assert_eq!(grid.target_rows(), vec![0, 1, 2]);
        grid.toggle_row_selection();
        assert_eq!(grid.target_rows(), vec![1, 2]);
    }

    #[test]
    fn staged_edits_build_guarded_change_set() {
        let mut grid = editable();
        grid.move_columns(1);
        grid.begin_edit().unwrap();
        let editor = grid.editing.as_mut().unwrap();
        editor.input.set("ada lovelace");
        grid.commit_edit();
        grid.move_rows(1);
        grid.delete_target_rows().unwrap();
        grid.insert_row().unwrap();
        grid.begin_edit().unwrap();
        grid.editing.as_mut().unwrap().input.set("new");
        grid.commit_edit();
        assert_eq!(grid.pending_count(), 3);
        let changes = grid.build_change_set(&table()).unwrap();
        assert_eq!(
            changes.changes,
            vec![
                RowChange::Update {
                    key: vec![("id".into(), Value::Int(1))],
                    cells: vec![CellChange {
                        column: "name".into(),
                        original: Value::Text("ada".into()),
                        updated: Value::Text("ada lovelace".into()),
                    }],
                },
                RowChange::Delete {
                    key: vec![("id".into(), Value::Int(2))]
                },
                RowChange::Insert {
                    values: vec![("name".into(), Value::Text("new".into()))]
                },
            ]
        );
        assert_eq!(grid.change_summary().len(), 3);
    }

    #[test]
    fn editing_back_to_original_clears_change_and_revert_works() {
        let mut grid = editable();
        grid.move_columns(1);
        grid.begin_edit().unwrap();
        grid.editing.as_mut().unwrap().input.set("zed");
        grid.commit_edit();
        assert!(grid.has_pending_changes());
        grid.begin_edit().unwrap();
        grid.editing.as_mut().unwrap().input.set("ada");
        grid.commit_edit();
        assert!(!grid.has_pending_changes());
        grid.set_null().unwrap();
        assert_eq!(grid.current_value(), Some(&Value::Null));
        grid.revert_current();
        assert!(!grid.has_pending_changes());
        grid.delete_target_rows().unwrap();
        grid.revert_current();
        assert!(!grid.has_pending_changes());
    }

    #[test]
    fn read_only_grids_refuse_edits() {
        let mut grid =
            GridState::from_batch(&batch(), Editability::ReadOnly("no primary key".into()));
        assert_eq!(grid.begin_edit(), Err("no primary key".into()));
        assert!(grid.insert_row().is_err());
        assert!(grid.build_change_set(&table()).is_err());
    }

    #[test]
    fn replace_rows_keeps_layout() {
        let mut grid = editable();
        grid.resize_column(5);
        let width = grid.columns[0].width;
        grid.hide_columns_named(&["note".to_owned()]);
        grid.replace_rows(&batch());
        assert_eq!(grid.columns[0].width, width);
        assert_eq!(grid.hidden_column_names(), vec!["note"]);
    }

    #[test]
    fn copy_and_json_export_use_visible_columns() {
        let mut grid = editable();
        grid.hide_columns_named(&["note".to_owned()]);
        assert_eq!(grid.copy_target_rows(), "id\tname\n1\tada");
        let json: serde_json::Value = serde_json::from_str(&grid.json_text()).unwrap();
        assert_eq!(json[1], serde_json::json!({"id": 2, "name": "bob"}));
    }
}
