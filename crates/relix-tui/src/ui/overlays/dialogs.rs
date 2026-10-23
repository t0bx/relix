/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::text;
use crate::ui::UiState;
use crate::ui::widgets::{bold, centered, input, popup, put};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use relix_app::overlay::{ConfirmState, PromptState, TextEditState, ViewerState};

pub fn confirm(frame: &mut Frame<'_>, area: Rect, confirm: &ConfirmState, ui: &UiState) {
    let styles = &ui.styles;
    let width = confirm
        .lines
        .iter()
        .map(|line| text::width(line) as u16)
        .max()
        .unwrap_or(30)
        .clamp(40, 100)
        + 4;
    let outer = centered(area, width, confirm.lines.len() as u16 + 5);
    let inner = popup(frame, styles, outer, &confirm.title);
    let buffer = frame.buffer_mut();
    for (row, line) in confirm.lines.iter().enumerate() {
        put(
            buffer,
            inner.x + 1,
            inner.y + row as u16,
            line,
            inner.width.saturating_sub(2),
            styles.panel(),
        );
    }
    let button_color = if confirm.danger {
        styles.error
    } else {
        styles.accent
    };
    let y = inner.y + inner.height.saturating_sub(1);
    let x = put(
        buffer,
        inner.x + 1,
        y,
        &format!(" Enter {} ", confirm.confirm_label),
        inner.width,
        bold(Style::default().fg(styles.background).bg(button_color)),
    );
    put(
        buffer,
        x + 2,
        y,
        " Esc Cancel ",
        inner.width,
        Style::default()
            .fg(styles.foreground)
            .bg(styles.cursor_line),
    );
}

pub fn prompt(frame: &mut Frame<'_>, area: Rect, prompt: &PromptState, ui: &UiState) {
    let styles = &ui.styles;
    let outer = centered(area, 72, 5);
    let inner = popup(frame, styles, outer, &prompt.title);
    let buffer = frame.buffer_mut();
    let field = Style::default()
        .fg(styles.foreground)
        .bg(styles.cursor_line);
    let position = input(
        buffer,
        Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), 1),
        &prompt.input,
        field,
        prompt.masked,
    );
    put(
        buffer,
        inner.x + 1,
        inner.y + 2,
        &prompt.hint,
        inner.width.saturating_sub(2),
        styles.muted(),
    );
    frame.set_cursor_position(position);
}

pub fn viewer(frame: &mut Frame<'_>, area: Rect, viewer: &ViewerState, ui: &UiState) {
    let styles = &ui.styles;
    let height = (viewer.lines.len() as u16 + 3).clamp(6, area.height.saturating_sub(4));
    let outer = centered(area, area.width.saturating_sub(8).min(110), height);
    let inner = popup(frame, styles, outer, &viewer.title);
    let buffer = frame.buffer_mut();
    let rows = usize::from(inner.height.saturating_sub(1));
    for (offset, line) in viewer
        .lines
        .iter()
        .skip(viewer.scroll)
        .take(rows)
        .enumerate()
    {
        put(
            buffer,
            inner.x + 1,
            inner.y + offset as u16,
            line,
            inner.width.saturating_sub(2),
            styles.panel(),
        );
    }
    let footer = format!(
        "{}/{} · ↑↓ scroll · y copy · Esc close",
        (viewer.scroll + rows).min(viewer.lines.len()),
        viewer.lines.len()
    );
    put(
        buffer,
        inner.x + 1,
        inner.y + inner.height.saturating_sub(1),
        &footer,
        inner.width,
        styles.muted(),
    );
}

pub fn text_edit(frame: &mut Frame<'_>, area: Rect, edit: &TextEditState, ui: &UiState) {
    let styles = &ui.styles;
    let outer = centered(
        area,
        area.width.saturating_sub(10).min(100),
        area.height.saturating_sub(6).min(24),
    );
    let inner = popup(frame, styles, outer, &edit.title);
    let rows = usize::from(inner.height.saturating_sub(1));
    let cursor = edit.buffer.cursor();
    let scroll = cursor.line.saturating_sub(rows.saturating_sub(1));
    let selection = edit.buffer.selection();
    let buffer = frame.buffer_mut();
    for (offset, line) in edit
        .buffer
        .lines()
        .iter()
        .skip(scroll)
        .take(rows)
        .enumerate()
    {
        let line_index = scroll + offset;
        let y = inner.y + offset as u16;
        let mut x = inner.x + 1;
        for (column, ch) in line.chars().enumerate() {
            let position = relix_editor::Position::new(line_index, column);
            let selected =
                selection.is_some_and(|(start, end)| position >= start && position < end);
            let style = if selected {
                styles.selected()
            } else {
                styles.panel()
            };
            let width = text::width(&ch.to_string()) as u16;
            if x + width >= inner.x + inner.width {
                break;
            }
            buffer.set_stringn(x, y, ch.to_string(), usize::from(width), style);
            x += width;
        }
    }
    put(
        buffer,
        inner.x + 1,
        inner.y + inner.height.saturating_sub(1),
        "Ctrl+S save · Ctrl+Z undo · Esc cancel",
        inner.width,
        styles.muted(),
    );
    let line = edit.buffer.line(cursor.line);
    let x = inner.x + 1 + text::width_of_prefix(line, cursor.column) as u16;
    let y = inner.y + (cursor.line - scroll) as u16;
    frame.set_cursor_position(Position::new(
        x.min(inner.x + inner.width.saturating_sub(1)),
        y,
    ));
}
