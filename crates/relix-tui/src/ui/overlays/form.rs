/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::ui::UiState;
use crate::ui::widgets::{bold, centered, input, popup, put};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use relix_app::form::{FieldValue, FormView};

const LABEL_WIDTH: u16 = 17;

pub fn draw(frame: &mut Frame<'_>, area: Rect, form: &dyn FormView, ui: &UiState) {
    let styles = &ui.styles;
    let rows = form.rows();
    let outer = centered(area, 92, rows.len() as u16 + 5);
    let inner = popup(frame, styles, outer, &form.title());
    let visible_rows = usize::from(inner.height.saturating_sub(2));
    let focused_row = rows.iter().position(|row| row.focused).unwrap_or(0);
    let skip = focused_row.saturating_sub(visible_rows.saturating_sub(1));
    let mut cursor = None;
    for (offset, row) in rows.iter().skip(skip).take(visible_rows).enumerate() {
        let y = inner.y + offset as u16;
        let buffer = frame.buffer_mut();
        let label_style = if row.focused {
            bold(Style::default().fg(styles.accent))
        } else {
            styles.muted()
        };
        put(buffer, inner.x + 1, y, row.label, LABEL_WIDTH, label_style);
        let value_area = Rect::new(
            inner.x + LABEL_WIDTH + 2,
            y,
            inner.width.saturating_sub(LABEL_WIDTH + 3),
            1,
        );
        let value_style = if row.focused {
            Style::default()
                .fg(styles.foreground)
                .bg(styles.cursor_line)
        } else {
            styles.panel()
        };
        match row.value {
            FieldValue::Text(text) | FieldValue::Secret(text) => {
                let masked = matches!(row.value, FieldValue::Secret(_));
                let position = input(buffer, value_area, text, value_style, masked);
                if let Some(placeholder) = row.placeholder.filter(|_| text.is_empty()) {
                    put(
                        buffer,
                        value_area.x,
                        y,
                        placeholder,
                        value_area.width,
                        value_style.fg(styles.muted),
                    );
                }
                if row.focused {
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
    if let Some(status) = form.status() {
        let color = if form.busy() {
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
        form.footer(),
        inner.width.saturating_sub(2),
        styles.muted(),
    );
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }
}
