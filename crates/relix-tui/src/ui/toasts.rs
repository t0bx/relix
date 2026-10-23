/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::UiState;
use super::widgets::put;
use crate::text;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use relix_app::App;
use relix_app::notifications::Level;

const TOAST_WIDTH: u16 = 56;

pub fn draw(frame: &mut Frame<'_>, area: Rect, app: &App, ui: &UiState) {
    let styles = &ui.styles;
    let width = TOAST_WIDTH.min(area.width.saturating_sub(2));
    let x = area.x + area.width - width - 1;
    let bottom = area.y + area.height.saturating_sub(2);
    for (y, notification) in (area.y + 1..bottom)
        .rev()
        .zip(app.notifications.items().iter().rev())
    {
        let (icon, color) = match notification.level {
            Level::Info => ("ℹ", styles.info),
            Level::Success => ("✓", styles.success),
            Level::Warning => ("!", styles.warning),
            Level::Error => ("✗", styles.error),
        };
        let message = format!(" {icon} {}", notification.message);
        let style = Style::default().fg(styles.foreground).bg(styles.surface);
        let buffer = frame.buffer_mut();
        buffer.set_style(Rect::new(x, y, width, 1), style);
        put(
            buffer,
            x,
            y,
            "▌",
            1,
            Style::default().fg(color).bg(styles.surface),
        );
        put(
            buffer,
            x + 1,
            y,
            &text::fit(&message, usize::from(width - 1)),
            width - 1,
            style.fg(color),
        );
    }
}
