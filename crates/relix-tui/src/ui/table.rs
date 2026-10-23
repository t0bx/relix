/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::UiState;
use super::grid::{self, GridFrame};
use super::widgets::{bold, input, put};
use crate::text;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use relix_app::App;
use relix_app::grid::Editability;
use relix_app::tab::Tab;
use relix_app::table_tab::TableFocus;
use relix_app::workspace::{PaneId, TabId};
use relix_core::SortDirection;

pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    tab_id: TabId,
    pane: PaneId,
    focused: bool,
    app: &mut App,
    ui: &UiState,
) {
    let styles = &ui.styles;
    let overlays_open = !app.overlays.is_empty();
    let App {
        workspace, hit_map, ..
    } = app;
    let Some(Tab::Table(table)) = workspace.tab_mut(tab_id) else {
        return;
    };
    if area.height < 4 {
        return;
    }
    let filter_area = Rect::new(area.x, area.y, area.width, 1);
    let footer_area = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
    let grid_area = Rect::new(area.x, area.y + 1, area.width, area.height - 2);
    let filter_focused = focused && table.focus == TableFocus::Filter;
    let bar_style = Style::default().fg(styles.foreground).bg(styles.surface);
    let buffer = frame.buffer_mut();
    buffer.set_style(filter_area, bar_style);
    put(
        buffer,
        area.x,
        area.y,
        " WHERE ",
        7,
        bold(bar_style.fg(if filter_focused {
            styles.accent
        } else {
            styles.muted
        })),
    );
    let order = order_summary(&table.query.sort);
    let order_width = (text::width(&order) as u16).min(area.width / 3);
    let input_width = area.width.saturating_sub(8 + order_width);
    let mut filter_cursor = None;
    if table.query.filter_input.is_empty() && !filter_focused {
        put(
            buffer,
            area.x + 7,
            area.y,
            "press w to filter…",
            input_width,
            bar_style.fg(styles.muted),
        );
    } else {
        filter_cursor = Some(input(
            buffer,
            Rect::new(area.x + 7, area.y, input_width, 1),
            &table.query.filter_input,
            bar_style,
            false,
        ));
    }
    put(
        buffer,
        area.x + area.width - order_width,
        area.y,
        &text::fit_right(&order, usize::from(order_width)),
        order_width,
        bar_style.fg(styles.accent_secondary),
    );
    let mut cursor = None;
    match table.grid.as_mut() {
        Some(grid) => {
            let info = GridFrame {
                sort: &table.query.sort,
                focused: focused && table.focus == TableFocus::Grid,
                pane,
                row_number_offset: table.query.offset,
            };
            cursor = grid::draw(frame, grid_area, grid, &info, hit_map, ui);
        }
        None => {
            let message = match (&table.error, table.loading) {
                (Some(error), _) => (error.clone(), styles.error),
                (None, true) => (
                    format!("{} loading {}…", ui.spinner(), table.table),
                    styles.muted,
                ),
                (None, false) => ("no data".to_owned(), styles.muted),
            };
            put(
                frame.buffer_mut(),
                grid_area.x + 1,
                grid_area.y + 1,
                &message.0,
                grid_area.width,
                Style::default().fg(message.1),
            );
        }
    }
    let footer = footer_text(table, ui);
    let buffer = frame.buffer_mut();
    buffer.set_style(footer_area, Style::default().bg(styles.surface));
    put(
        buffer,
        footer_area.x,
        footer_area.y,
        &footer.0,
        footer_area.width,
        Style::default().fg(footer.1).bg(styles.surface),
    );
    if filter_focused && let Some(completion) = table.completion.as_ref() {
        let rows = completion.items.len().min(8) as u16;
        let width = 44.min(area.width);
        let x = filter_cursor
            .map_or(area.x + 7, |position| position.x)
            .min((area.x + area.width).saturating_sub(width));
        for (offset, item) in completion.items.iter().take(usize::from(rows)).enumerate() {
            let selected = offset == completion.selected;
            let style = if selected {
                styles.selected()
            } else {
                Style::default().fg(styles.foreground).bg(styles.surface)
            };
            let y = area.y + 1 + offset as u16;
            buffer.set_style(Rect::new(x, y, width, 1), style);
            let line = format!(" {:<5} {}", item.kind.label(), item.label);
            put(
                buffer,
                x,
                y,
                &text::fit(&line, usize::from(width)),
                width,
                style,
            );
        }
    }
    if focused && !overlays_open {
        let position = if filter_focused {
            filter_cursor
        } else {
            cursor
        };
        if let Some(position) = position {
            frame.set_cursor_position(position);
        }
    }
}

fn order_summary(sort: &[relix_core::SortKey]) -> String {
    if sort.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = sort
        .iter()
        .map(|key| {
            let arrow = match key.direction {
                SortDirection::Ascending => "▲",
                SortDirection::Descending => "▼",
            };
            format!("{} {arrow}", key.column)
        })
        .collect();
    format!("ORDER BY {} ", parts.join(", "))
}

fn footer_text(
    table: &relix_app::table_tab::TableTab,
    ui: &UiState,
) -> (String, ratatui::style::Color) {
    let styles = &ui.styles;
    let Some(grid) = table.grid.as_ref() else {
        return (String::new(), styles.muted);
    };
    let mut parts = vec![format!(" rows {}", table.query.page_label(grid.rows.len()))];
    if table.loading {
        parts.push(format!("{} loading", ui.spinner()));
    }
    let pending = grid.pending_count();
    let color = if pending > 0 {
        parts.push(format!(
            "{pending} pending change(s) — Ctrl+S review & commit, U discard"
        ));
        styles.warning
    } else {
        styles.muted
    };
    match &grid.editability {
        Editability::ReadOnly(reason) => parts.push(format!("read-only: {reason}")),
        Editability::Editable { key_columns } => {
            parts.push(format!("editable · key ({})", key_columns.join(", ")))
        }
    }
    if let Some(error) = &table.error {
        return (format!(" {error}"), styles.error);
    }
    (parts.join(" · "), color)
}
