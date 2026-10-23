/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::ui::UiState;
use crate::ui::widgets::{centered, popup, put};
use ratatui::Frame;
use ratatui::layout::Rect;
use relix_app::App;
use relix_app::overlay::{ColumnChooserState, SortEditorState};
use relix_app::tab::Tab;
use relix_core::SortDirection;

pub fn sort_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    editor: &SortEditorState,
    app: &App,
    ui: &UiState,
) {
    let styles = &ui.styles;
    let Some(Tab::Table(table)) = app.workspace.tab(editor.tab) else {
        return;
    };
    let rules = &table.query.sort;
    let outer = centered(area, 56, rules.len().max(1) as u16 + 4);
    let inner = popup(frame, styles, outer, "Sort rules");
    let buffer = frame.buffer_mut();
    if rules.is_empty() {
        put(
            buffer,
            inner.x + 1,
            inner.y,
            "no sort rules — use s / S on a column",
            inner.width,
            styles.muted(),
        );
    }
    for (index, rule) in rules.iter().enumerate() {
        let style = if index == editor.selected {
            styles.selected()
        } else {
            styles.panel()
        };
        let direction = match rule.direction {
            SortDirection::Ascending => "▲ ascending",
            SortDirection::Descending => "▼ descending",
        };
        let y = inner.y + index as u16;
        buffer.set_style(Rect::new(inner.x, y, inner.width, 1), style);
        put(
            buffer,
            inner.x + 1,
            y,
            &format!("{}. {:<24} {direction}", index + 1, rule.column),
            inner.width,
            style,
        );
    }
    put(
        buffer,
        inner.x + 1,
        inner.y + inner.height.saturating_sub(1),
        "Space flip · d remove · J/K move · Esc close",
        inner.width,
        styles.muted(),
    );
}

pub fn column_chooser(
    frame: &mut Frame<'_>,
    area: Rect,
    chooser: &ColumnChooserState,
    app: &App,
    ui: &UiState,
) {
    let styles = &ui.styles;
    let grid = match app.workspace.tab(chooser.tab) {
        Some(Tab::Table(table)) => table.grid.as_ref(),
        Some(Tab::Editor(editor)) => editor
            .active_result()
            .and_then(|result| match &result.kind {
                relix_app::editor_tab::ResultKind::Rows { grid, .. } => Some(&**grid),
                _ => None,
            }),
        _ => None,
    };
    let Some(grid) = grid else {
        return;
    };
    let rows = grid.columns.len().min(20) as u16;
    let outer = centered(area, 50, rows + 3);
    let inner = popup(frame, styles, outer, "Columns");
    let list = usize::from(inner.height.saturating_sub(1));
    let first = chooser.selected.saturating_sub(list.saturating_sub(1));
    let buffer = frame.buffer_mut();
    for (offset, column) in grid.columns.iter().enumerate().skip(first).take(list) {
        let y = inner.y + (offset - first) as u16;
        let style = if offset == chooser.selected {
            styles.selected()
        } else {
            styles.panel()
        };
        buffer.set_style(Rect::new(inner.x, y, inner.width, 1), style);
        let mark = if column.hidden { "[ ]" } else { "[x]" };
        let kind = column.meta.type_name.clone().unwrap_or_default();
        put(
            buffer,
            inner.x + 1,
            y,
            &format!("{mark} {:<24} {kind}", column.meta.name),
            inner.width,
            style,
        );
    }
    put(
        buffer,
        inner.x + 1,
        inner.y + inner.height.saturating_sub(1),
        "Space toggle · a show all · Esc close",
        inner.width,
        styles.muted(),
    );
}
