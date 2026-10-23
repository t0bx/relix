/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::text;
use crate::ui::UiState;
use crate::ui::widgets::{bold, centered, popup, put};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use relix_app::App;
use relix_app::command::{KeyContext, spec_for};

/// Lists every command with its keys, grouped by where the keys apply; the groups that
/// are active in the current focus come first.
pub fn draw(frame: &mut Frame<'_>, area: Rect, scroll: usize, app: &App, ui: &UiState) {
    let styles = &ui.styles;
    let active = app.key_contexts();
    let mut groups: Vec<KeyContext> = active.clone();
    for context in [
        KeyContext::Global,
        KeyContext::NonText,
        KeyContext::Navigable,
        KeyContext::Explorer,
        KeyContext::Editor,
        KeyContext::Grid,
        KeyContext::KeyValue,
        KeyContext::Document,
    ] {
        if !groups.contains(&context) {
            groups.push(context);
        }
    }
    let mut lines: Vec<(String, bool)> = Vec::new();
    for context in groups {
        let bindings = app.keymap.bindings_in(context);
        if bindings.is_empty() {
            continue;
        }
        let marker = if active.contains(&context) {
            " (active)"
        } else {
            ""
        };
        lines.push((format!("{}{marker}", context_title(context)), true));
        let mut seen = Vec::new();
        for (_, command) in bindings {
            let Some(spec) = spec_for(command) else {
                continue;
            };
            if seen.contains(&spec.title) {
                continue;
            }
            seen.push(spec.title);
            let all_keys: Vec<String> = app
                .keymap
                .bindings_in(context)
                .into_iter()
                .filter(|(_, other)| *other == command)
                .map(|(keys, _)| keys.to_string())
                .collect();
            lines.push((
                format!("  {}  {}", text::fit(&all_keys.join(", "), 26), spec.title),
                false,
            ));
        }
        lines.push((String::new(), false));
    }
    let outer = centered(area, 86, area.height.saturating_sub(4));
    let inner = popup(frame, styles, outer, "Keyboard shortcuts");
    let rows = usize::from(inner.height.saturating_sub(1));
    let scroll = scroll.min(lines.len().saturating_sub(rows));
    let buffer = frame.buffer_mut();
    for (offset, (line, heading)) in lines.iter().skip(scroll).take(rows).enumerate() {
        let style = if *heading {
            bold(Style::default().fg(styles.accent))
        } else {
            styles.panel()
        };
        put(
            buffer,
            inner.x + 1,
            inner.y + offset as u16,
            line,
            inner.width.saturating_sub(2),
            style,
        );
    }
    put(
        buffer,
        inner.x + 1,
        inner.y + inner.height.saturating_sub(1),
        "↑↓ scroll · Esc close · keys are configurable in config.toml [keybindings]",
        inner.width,
        styles.muted(),
    );
}

fn context_title(context: KeyContext) -> &'static str {
    match context {
        KeyContext::Global => "Global",
        KeyContext::NonText => "Outside text inputs",
        KeyContext::Navigable => "Lists & grids",
        KeyContext::Explorer => "Database explorer",
        KeyContext::Editor => "SQL editor",
        KeyContext::Grid => "Data grid",
        KeyContext::KeyValue => "Key browser",
        KeyContext::Document => "Document browser",
    }
}

/// Which-key popup: once a prefix such as the leader key is pressed, lists every key
/// that can follow it in the current focus.
pub fn which_key(frame: &mut Frame<'_>, area: Rect, app: &App, ui: &UiState) {
    let styles = &ui.styles;
    let continuations = app
        .keymap
        .continuations(&app.key_contexts(), &app.pending_keys);
    if continuations.is_empty() {
        return;
    }
    let rows = continuations.len().div_ceil(2) as u16;
    let width = 72.min(area.width);
    let outer = Rect::new(
        area.x + area.width - width - 1,
        area.y + area.height.saturating_sub(rows + 4),
        width,
        rows + 2,
    );
    let prefix: Vec<String> = app.pending_keys.iter().map(ToString::to_string).collect();
    let inner = popup(frame, styles, outer, &prefix.join(" "));
    let column_width = inner.width / 2;
    let buffer = frame.buffer_mut();
    for (index, continuation) in continuations.iter().enumerate() {
        let x = inner.x + (index % 2) as u16 * column_width;
        let y = inner.y + (index / 2) as u16;
        let title = spec_for(continuation.command).map_or("?", |spec| spec.title);
        let x = put(
            buffer,
            x + 1,
            y,
            &format!(" {} ", continuation.key),
            6,
            bold(Style::default().fg(styles.background).bg(styles.accent)),
        );
        put(
            buffer,
            x + 1,
            y,
            title,
            column_width.saturating_sub(8),
            styles.panel(),
        );
    }
}
