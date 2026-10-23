/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::UiState;
use super::widgets::{bold, fill_line, input, put};
use crate::text;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use relix_app::App;
use relix_app::document_tab::{DocumentFocus, DocumentTab};
use relix_app::tab::Tab;
use relix_app::workspace::TabId;

pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    tab_id: TabId,
    focused: bool,
    app: &App,
    ui: &UiState,
) {
    let Some(Tab::Document(tab)) = app.workspace.tab(tab_id) else {
        return;
    };
    if area.height < 4 {
        return;
    }
    let [filter_area, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(2),
        Constraint::Length(1),
    ])
    .areas(area);
    let cursor = draw_filter(frame, filter_area, tab, focused, ui);
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).areas(body);
    draw_list(frame, list_area, tab, focused, ui);
    draw_detail(frame, detail_area, tab, ui);
    draw_footer(frame, footer, tab, ui);
    if focused
        && app.overlays.is_empty()
        && tab.focus == DocumentFocus::Filter
        && let Some(position) = cursor
    {
        frame.set_cursor_position(position);
    }
}

fn draw_filter(
    frame: &mut Frame<'_>,
    area: Rect,
    tab: &DocumentTab,
    focused: bool,
    ui: &UiState,
) -> Option<ratatui::layout::Position> {
    let styles = &ui.styles;
    let buffer = frame.buffer_mut();
    let bar = Style::default().fg(styles.foreground).bg(styles.surface);
    buffer.set_style(area, bar);
    let active = focused && tab.focus == DocumentFocus::Filter;
    let label_color = if active { styles.accent } else { styles.muted };
    put(
        buffer,
        area.x,
        area.y,
        " FILTER ",
        8,
        bold(bar.fg(label_color)),
    );
    let field = Rect::new(area.x + 9, area.y, area.width.saturating_sub(9), 1);
    let position = input(buffer, field, &tab.filter, bar, false);
    if tab.filter.is_empty() && !active {
        let hint = if tab.filter_hint.is_empty() {
            "press / to filter"
        } else {
            tab.filter_hint.as_str()
        };
        put(
            buffer,
            field.x,
            field.y,
            hint,
            field.width,
            bar.fg(styles.muted),
        );
    }
    Some(position)
}

fn draw_list(frame: &mut Frame<'_>, area: Rect, tab: &DocumentTab, focused: bool, ui: &UiState) {
    let styles = &ui.styles;
    let buffer = frame.buffer_mut();
    if tab.documents.is_empty() {
        let message = if tab.loading.is_some() {
            "loading…"
        } else {
            "no documents"
        };
        put(
            buffer,
            area.x + 1,
            area.y,
            message,
            area.width,
            styles.muted(),
        );
        return;
    }
    let height = usize::from(area.height);
    let first = tab.selected.saturating_sub(height.saturating_sub(1));
    for (row, document) in tab.documents.iter().enumerate().skip(first).take(height) {
        let y = area.y + (row - first) as u16;
        let selected = row == tab.selected;
        let style = match (selected, focused) {
            (true, true) => styles.selected(),
            (true, false) => Style::default().bg(styles.cursor_line),
            _ => styles.panel(),
        };
        if selected {
            fill_line(buffer, area, y, style);
        }
        let key = text::truncate(&document.key_label(), 24);
        let x = put(
            buffer,
            area.x + 1,
            y,
            &key,
            area.width.saturating_sub(1),
            bold(style.fg(styles.accent)),
        );
        let preview = text::truncate(&document.body.to_string(), usize::from(area.width));
        put(
            buffer,
            x + 1,
            y,
            &preview,
            (area.x + area.width).saturating_sub(x + 1),
            style.fg(styles.muted),
        );
    }
}

fn draw_detail(frame: &mut Frame<'_>, area: Rect, tab: &DocumentTab, ui: &UiState) {
    let styles = &ui.styles;
    let buffer = frame.buffer_mut();
    let x = area.x + 1;
    let width = area.width.saturating_sub(2);
    if let Some(error) = &tab.error {
        put(
            buffer,
            x,
            area.y,
            error,
            width,
            Style::default().fg(styles.error),
        );
        return;
    }
    let Some(json) = tab.selected_json() else {
        return;
    };
    for (row, line) in json
        .lines()
        .skip(tab.detail_scroll)
        .take(usize::from(area.height))
        .enumerate()
    {
        let color = if line.trim_start().starts_with('"') {
            styles.syntax_string
        } else {
            styles.foreground
        };
        put(
            buffer,
            x,
            area.y + row as u16,
            line,
            width,
            Style::default().fg(color),
        );
    }
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, tab: &DocumentTab, ui: &UiState) {
    let styles = &ui.styles;
    let label = format!(
        " {} · {} · {}  a insert · e edit · d delete · i details · +/- index",
        tab.collection,
        tab.kind.label(),
        tab.page_label()
    );
    put(
        frame.buffer_mut(),
        area.x,
        area.y,
        &label,
        area.width,
        styles.muted(),
    );
}
