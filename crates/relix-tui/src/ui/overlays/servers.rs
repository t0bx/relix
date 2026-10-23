/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::ui::UiState;
use crate::ui::widgets::{bold, centered, popup, put};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use relix_app::App;
use relix_app::server_manager::{SERVER_MANAGER_HINTS, ServerKind, ServerManagerState};

pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &ServerManagerState,
    app: &App,
    ui: &UiState,
) {
    let styles = &ui.styles;
    let rows = app.server_rows();
    let sections = 2 + u16::from(!rows.is_empty());
    let outer = centered(area, 110, rows.len() as u16 + sections + 6);
    let inner = popup(frame, styles, outer, "Server configurations");
    let selected = state.selected.min(rows.len().saturating_sub(1));
    let buffer = frame.buffer_mut();
    let mut y = inner.y;
    let bottom = inner.y + inner.height.saturating_sub(2);
    for kind in [ServerKind::Ssh, ServerKind::Vpn] {
        if y >= bottom {
            break;
        }
        put(
            buffer,
            inner.x + 1,
            y,
            kind.label(),
            inner.width,
            bold(Style::default().fg(styles.accent)),
        );
        y += 1;
        let mut any = false;
        for (index, row) in rows.iter().enumerate().filter(|(_, row)| row.kind == kind) {
            if y >= bottom {
                break;
            }
            any = true;
            let style = if index == selected {
                styles.selected()
            } else {
                styles.panel()
            };
            buffer.set_style(Rect::new(inner.x, y, inner.width, 1), style);
            let usage = match row.usages {
                0 => "unused".to_owned(),
                1 => "1 data source".to_owned(),
                count => format!("{count} data sources"),
            };
            let text = format!(
                "{} {:<26} {:<8} {:<40} {:<14} {}",
                if index + 1 == rows.len()
                    || rows.get(index + 1).is_some_and(|next| next.kind != kind)
                {
                    "└──"
                } else {
                    "├──"
                },
                row.name,
                row.scope.label(),
                row.detail,
                usage,
                row.status
            );
            put(
                buffer,
                inner.x + 1,
                y,
                &text,
                inner.width.saturating_sub(2),
                style,
            );
            y += 1;
        }
        if !any && y < bottom {
            let hint = match kind {
                ServerKind::Ssh => "└── none yet — press n to create an SSH configuration",
                ServerKind::Vpn => "└── none yet — press v to create a VPN profile",
            };
            put(buffer, inner.x + 1, y, hint, inner.width, styles.muted());
            y += 1;
        }
        y += 1;
    }
    if let Some(message) = &state.message {
        put(
            buffer,
            inner.x + 1,
            inner.y + inner.height.saturating_sub(2),
            message,
            inner.width.saturating_sub(2),
            Style::default().fg(styles.info),
        );
    }
    put(
        buffer,
        inner.x + 1,
        inner.y + inner.height.saturating_sub(1),
        SERVER_MANAGER_HINTS,
        inner.width.saturating_sub(2),
        styles.muted(),
    );
}
