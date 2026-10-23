/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::ui::UiState;
use crate::ui::widgets::{bold, put};
use ratatui::Frame;
use ratatui::style::Style;
use relix_app::overlay::JumpState;

/// Draws flash-jump labels on top of the positions recorded while rendering the frame.
pub fn draw(frame: &mut Frame<'_>, jump: &JumpState, ui: &UiState) {
    let styles = &ui.styles;
    let positions = ui.jump_positions.borrow();
    let area = frame.area();
    for (label, target) in &jump.targets {
        if !label.starts_with(&jump.typed) {
            continue;
        }
        let Some((_, position)) = positions.iter().find(|(candidate, _)| candidate == target)
        else {
            continue;
        };
        let style = bold(Style::default().fg(styles.background).bg(styles.warning));
        put(
            frame.buffer_mut(),
            position.x,
            position.y,
            label,
            area.width.saturating_sub(position.x),
            style,
        );
    }
}
