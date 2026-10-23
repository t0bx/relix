/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::colors::Styles;
use crate::text;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear};
use relix_app::input::LineInput;
use relix_app::workspace::Area;

pub fn panel<'a>(styles: &Styles, title: impl Into<Line<'a>>, focused: bool) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(styles.border_type())
        .border_style(styles.border(focused))
        .title(title)
        .style(styles.base())
}

pub fn title_line<'a>(styles: &Styles, text: &str, focused: bool) -> Line<'a> {
    Line::from(Span::styled(format!(" {text} "), styles.title(focused)))
}

pub fn popup(frame: &mut Frame<'_>, styles: &Styles, area: Rect, title: &str) -> Rect {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(styles.border_type())
        .border_style(Style::default().fg(styles.accent))
        .title(Line::from(Span::styled(
            format!(" {title} "),
            styles.title(true),
        )))
        .style(styles.panel());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 3,
        width,
        height,
    )
}

pub fn put(buffer: &mut Buffer, x: u16, y: u16, text: &str, width: u16, style: Style) -> u16 {
    if width == 0 {
        return x;
    }
    let (end, _) = buffer.set_stringn(x, y, text, usize::from(width), style);
    end
}

pub fn fill_line(buffer: &mut Buffer, area: Rect, y: u16, style: Style) {
    buffer.set_style(Rect::new(area.x, y, area.width, 1), style);
}

/// Draws a single-line input with horizontal scrolling and returns the cursor position.
pub fn input(
    buffer: &mut Buffer,
    area: Rect,
    input: &LineInput,
    style: Style,
    masked: bool,
) -> Position {
    let width = usize::from(area.width.max(1));
    let shown: String = if masked {
        "•".repeat(input.len())
    } else {
        input.text().to_owned()
    };
    let cursor_width = text::width_of_prefix(&shown, input.cursor());
    let skip_width = cursor_width.saturating_sub(width.saturating_sub(1));
    let mut skipped = 0;
    let mut skip_chars = 0;
    for ch in shown.chars() {
        if skipped >= skip_width {
            break;
        }
        skipped += text::width(&ch.to_string());
        skip_chars += 1;
    }
    let visible = text::visible_slice(&shown, skip_chars, width);
    buffer.set_style(area, style);
    buffer.set_stringn(area.x, area.y, &visible, width, style);
    Position::new(
        area.x + (cursor_width - skipped).min(width - 1) as u16,
        area.y,
    )
}

pub fn to_area(rect: Rect) -> Area {
    Area::new(rect.x, rect.y, rect.width, rect.height)
}

pub fn bold(style: Style) -> Style {
    style.add_modifier(Modifier::BOLD)
}

pub fn key_hint<'a>(styles: &Styles, key: &str, label: &str) -> Vec<Span<'a>> {
    vec![
        Span::styled(
            format!(" {key} "),
            Style::default().fg(styles.background).bg(styles.muted),
        ),
        Span::styled(format!(" {label}  "), Style::default().fg(styles.status_fg)),
    ]
}
