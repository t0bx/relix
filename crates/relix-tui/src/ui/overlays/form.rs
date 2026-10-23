/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::ui::UiState;
use crate::ui::widgets::{bold, centered, input, popup, put};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use relix_app::datasource_form::{DataSourceForm, FieldValue};

const LABEL_WIDTH: u16 = 15;

pub fn draw(frame: &mut Frame<'_>, area: Rect, form: &DataSourceForm, ui: &UiState) {
    let styles = &ui.styles;
    let visible = form.visible_indexes();
    let title = if form.editing.is_some() {
        "Edit data source"
    } else {
        "New data source"
    };
    let outer = centered(area, 90, visible.len() as u16 + 5);
    let inner = popup(frame, styles, outer, title);
    let mut cursor = None;
    for (row, index) in visible.iter().enumerate() {
        let Some(field) = form.fields.get(*index) else {
            continue;
        };
        let y = inner.y + row as u16;
        let focused = *index == form.focused;
        let buffer = frame.buffer_mut();
        let label_style = if focused {
            bold(Style::default().fg(styles.accent))
        } else {
            styles.muted()
        };
        put(
            buffer,
            inner.x + 1,
            y,
            field.label,
            LABEL_WIDTH,
            label_style,
        );
        let value_area = Rect::new(
            inner.x + LABEL_WIDTH + 2,
            y,
            inner.width.saturating_sub(LABEL_WIDTH + 3),
            1,
        );
        let value_style = if focused {
            Style::default()
                .fg(styles.foreground)
                .bg(styles.cursor_line)
        } else {
            styles.panel()
        };
        match &field.value {
            FieldValue::Text(text) => {
                let position = input(buffer, value_area, text, value_style, false);
                if focused {
                    cursor = Some(position);
                }
            }
            FieldValue::Secret(text) => {
                let position = input(buffer, value_area, text, value_style, true);
                if text.is_empty() && form.editing.is_some() && !form.password_touched {
                    put(
                        buffer,
                        value_area.x,
                        y,
                        "(unchanged)",
                        value_area.width,
                        value_style.fg(styles.muted),
                    );
                }
                if focused {
                    cursor = Some(position);
                }
            }
            FieldValue::Choice { options, selected } => {
                buffer.set_style(value_area, value_style);
                let current = options.get(*selected).cloned().unwrap_or_default();
                put(
                    buffer,
                    value_area.x,
                    y,
                    &format!("◂ {current} ▸"),
                    value_area.width,
                    value_style.fg(styles.accent_secondary),
                );
            }
            FieldValue::Toggle(value) => {
                buffer.set_style(value_area, value_style);
                put(
                    buffer,
                    value_area.x,
                    y,
                    if *value { "[x]" } else { "[ ]" },
                    3,
                    value_style.fg(styles.accent_secondary),
                );
            }
        }
    }
    let buffer = frame.buffer_mut();
    let status_y = inner.y + inner.height.saturating_sub(2);
    if let Some(status) = &form.status {
        let color = if form.testing {
            styles.warning
        } else if status.success {
            styles.success
        } else {
            styles.error
        };
        put(
            buffer,
            inner.x + 1,
            status_y,
            &status.message,
            inner.width.saturating_sub(2),
            Style::default().fg(color),
        );
    }
    put(
        buffer,
        inner.x + 1,
        inner.y + inner.height.saturating_sub(1),
        "Tab/↑↓ field · ←→ change · Space toggle · Ctrl+T test · Enter save · Esc cancel",
        inner.width.saturating_sub(2),
        styles.muted(),
    );
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }
}
