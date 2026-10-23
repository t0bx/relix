/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::text;
use crate::ui::UiState;
use crate::ui::widgets::{centered, input, popup, put};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use relix_app::overlay::PickerState;

pub fn draw(frame: &mut Frame<'_>, area: Rect, picker: &PickerState, ui: &UiState) {
    let styles = &ui.styles;
    let rows = picker.filtered.len().clamp(1, 14) as u16;
    let outer = centered(area, 84, rows + 5);
    let inner = popup(frame, styles, outer, &picker.title);
    let buffer = frame.buffer_mut();
    put(
        buffer,
        inner.x,
        inner.y,
        "❯ ",
        2,
        Style::default().fg(styles.accent),
    );
    let cursor = input(
        buffer,
        Rect::new(inner.x + 2, inner.y, inner.width.saturating_sub(2), 1),
        &picker.input,
        styles.panel(),
        false,
    );
    let list_height = usize::from(inner.height.saturating_sub(3));
    let first = picker
        .selected
        .saturating_sub(list_height.saturating_sub(1));
    for (offset, index) in picker
        .filtered
        .iter()
        .skip(first)
        .take(list_height)
        .enumerate()
    {
        let Some(item) = picker.items.get(*index) else {
            continue;
        };
        let y = inner.y + 2 + offset as u16;
        let selected = first + offset == picker.selected;
        let style = if selected {
            styles.selected()
        } else {
            styles.panel()
        };
        buffer.set_style(Rect::new(inner.x, y, inner.width, 1), style);
        let hint_width = (text::width(&item.hint) as u16 + 1).min(inner.width / 3);
        let detail_width = (inner.width / 3).min(28);
        let label_width = inner.width.saturating_sub(hint_width + detail_width + 2);
        put(
            buffer,
            inner.x + 1,
            y,
            &text::fit(&item.label, usize::from(label_width)),
            label_width,
            style,
        );
        put(
            buffer,
            inner.x + 1 + label_width,
            y,
            &text::fit(&item.detail, usize::from(detail_width)),
            detail_width,
            style.fg(styles.muted),
        );
        put(
            buffer,
            inner.x + inner.width - hint_width,
            y,
            &text::fit_right(&item.hint, usize::from(hint_width)),
            hint_width,
            style.fg(styles.accent_secondary),
        );
    }
    if picker.filtered.is_empty() {
        put(
            buffer,
            inner.x + 1,
            inner.y + 2,
            "no matches",
            inner.width,
            styles.muted(),
        );
    }
    let footer_y = inner.y + inner.height.saturating_sub(1);
    put(
        buffer,
        inner.x,
        footer_y,
        &picker.footer,
        inner.width,
        styles.muted(),
    );
    frame.set_cursor_position(cursor);
}
