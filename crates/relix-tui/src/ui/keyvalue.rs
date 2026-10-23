/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::UiState;
use super::widgets::{bold, input, put};
use crate::text;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Style;
use relix_app::keyvalue_tab::{KeyValueFocus, KeyValueTab, ValueContent};
use relix_app::tab::Tab;
use relix_app::workspace::{Area, PaneId, TabId};
use relix_app::{App, HitTarget};

const CONSOLE_HEIGHT: u16 = 8;

pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    tab_id: TabId,
    pane: PaneId,
    focused: bool,
    app: &mut App,
    ui: &UiState,
) {
    let overlays_open = !app.overlays.is_empty();
    let App {
        workspace, hit_map, ..
    } = app;
    let Some(Tab::KeyValue(tab)) = workspace.tab_mut(tab_id) else {
        return;
    };
    let console_height = CONSOLE_HEIGHT.min(area.height / 3).max(3);
    let [top, console] =
        Layout::vertical([Constraint::Min(4), Constraint::Length(console_height)]).areas(area);
    let [keys_area, value_area] =
        Layout::horizontal([Constraint::Percentage(36), Constraint::Percentage(64)]).areas(top);
    let mut cursor = draw_keys(frame, keys_area, tab, pane, focused, hit_map, ui);
    draw_value(frame, value_area, tab, focused, ui);
    if let Some(position) = draw_console(frame, console, tab, focused, ui) {
        cursor = Some(position);
    }
    if focused
        && !overlays_open
        && let Some(position) = cursor
    {
        frame.set_cursor_position(position);
    }
}

fn draw_keys(
    frame: &mut Frame<'_>,
    area: Rect,
    tab: &mut KeyValueTab,
    pane: PaneId,
    focused: bool,
    hit_map: &mut relix_app::app::HitMap,
    ui: &UiState,
) -> Option<Position> {
    let styles = &ui.styles;
    let buffer = frame.buffer_mut();
    let bar_style = Style::default().fg(styles.foreground).bg(styles.surface);
    buffer.set_style(Rect::new(area.x, area.y, area.width, 1), bar_style);
    let pattern_focused = focused && tab.focus == KeyValueFocus::Pattern;
    put(
        buffer,
        area.x,
        area.y,
        " MATCH ",
        7,
        bold(bar_style.fg(if pattern_focused {
            styles.accent
        } else {
            styles.muted
        })),
    );
    let pattern_cursor = input(
        buffer,
        Rect::new(area.x + 7, area.y, area.width.saturating_sub(8), 1),
        &tab.pattern,
        bar_style,
        false,
    );
    let height = usize::from(area.height.saturating_sub(2));
    if tab.selected_key < tab.key_scroll {
        tab.key_scroll = tab.selected_key;
    } else if tab.selected_key >= tab.key_scroll + height.max(1) {
        tab.key_scroll = tab.selected_key + 1 - height.max(1);
    }
    let keys_focused = focused && tab.focus == KeyValueFocus::Keys;
    for (offset, key) in tab
        .keys
        .iter()
        .enumerate()
        .skip(tab.key_scroll)
        .take(height)
    {
        let y = area.y + 1 + (offset - tab.key_scroll) as u16;
        let selected = offset == tab.selected_key;
        let style = match (selected, keys_focused) {
            (true, true) => styles.selected(),
            (true, false) => Style::default()
                .fg(styles.foreground)
                .bg(styles.cursor_line),
            _ => Style::default().fg(styles.foreground),
        };
        buffer.set_style(Rect::new(area.x, y, area.width, 1), style);
        put(
            buffer,
            area.x + 1,
            y,
            &key.display(),
            area.width.saturating_sub(2),
            style,
        );
        hit_map.push(
            Area::new(area.x, y, area.width, 1),
            HitTarget::KeyRow { pane, row: offset },
        );
    }
    let status = if tab.scanning.is_some() {
        format!(" {} scanning… {} keys", ui.spinner(), tab.keys.len())
    } else if tab.scan_complete {
        format!(" {} keys", tab.keys.len())
    } else {
        format!(" {} keys loaded · m: more", tab.keys.len())
    };
    let y = area.y + area.height.saturating_sub(1);
    buffer.set_style(
        Rect::new(area.x, y, area.width, 1),
        Style::default().bg(styles.surface),
    );
    put(
        buffer,
        area.x,
        y,
        &status,
        area.width,
        Style::default().fg(styles.muted).bg(styles.surface),
    );
    pattern_focused.then_some(pattern_cursor)
}

fn draw_value(
    frame: &mut Frame<'_>,
    area: Rect,
    tab: &mut KeyValueTab,
    focused: bool,
    ui: &UiState,
) {
    let styles = &ui.styles;
    let buffer = frame.buffer_mut();
    let header_style = Style::default().fg(styles.foreground).bg(styles.surface);
    buffer.set_style(Rect::new(area.x, area.y, area.width, 1), header_style);
    let Some(description) = tab.description.as_ref() else {
        let server: Vec<String> = tab
            .server_details
            .iter()
            .filter(|(key, _)| {
                key.ends_with("redis_version")
                    || key == "dbsize"
                    || key.ends_with("used_memory_human")
                    || key.ends_with("connected_clients")
            })
            .map(|(key, value)| format!("{}: {value}", key.rsplit('.').next().unwrap_or(key)))
            .collect();
        put(
            buffer,
            area.x + 1,
            area.y,
            &server.join(" · "),
            area.width,
            header_style.fg(styles.muted),
        );
        put(
            buffer,
            area.x + 2,
            area.y + 2,
            "select a key to inspect its value",
            area.width,
            styles.muted(),
        );
        return;
    };
    let mut info = vec![
        format!(" {} ", description.key_type.label().to_uppercase()),
        format!("TTL {}", description.ttl.label()),
    ];
    if let Some(length) = description.length {
        info.push(format!("len {length}"));
    }
    if let Some(encoding) = &description.encoding {
        info.push(encoding.clone());
    }
    if let Some(memory) = description.memory_bytes {
        info.push(format!("{memory} B"));
    }
    let x = put(
        buffer,
        area.x,
        area.y,
        &info[0],
        12,
        bold(
            Style::default()
                .fg(styles.background)
                .bg(styles.accent_secondary),
        ),
    );
    put(
        buffer,
        x + 1,
        area.y,
        &format!("{} · {}", description.key.display(), info[1..].join(" · ")),
        area.width.saturating_sub(x - area.x + 1),
        header_style,
    );
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    let value_focused = focused && tab.focus == KeyValueFocus::Value;
    match &tab.value {
        Some(ValueContent::Text {
            bytes,
            total_length,
        }) => {
            let content = String::from_utf8_lossy(bytes);
            let pretty = serde_pretty(&content).unwrap_or_else(|| content.into_owned());
            for (offset, line) in pretty.lines().take(usize::from(body.height)).enumerate() {
                put(
                    buffer,
                    body.x + 1,
                    body.y + offset as u16,
                    line,
                    body.width.saturating_sub(2),
                    Style::default().fg(styles.syntax_string),
                );
            }
            if bytes.len() as u64 != *total_length {
                let note = format!(" showing {} of {} bytes ", bytes.len(), total_length);
                put(
                    buffer,
                    body.x + body.width.saturating_sub(note.len() as u16),
                    body.y + body.height.saturating_sub(1),
                    &note,
                    body.width,
                    Style::default().fg(styles.warning),
                );
            }
        }
        Some(ValueContent::Items { items, next_cursor }) => {
            let height = usize::from(body.height.saturating_sub(1));
            if tab.selected_item < tab.item_scroll {
                tab.item_scroll = tab.selected_item;
            } else if tab.selected_item >= tab.item_scroll + height.max(1) {
                tab.item_scroll = tab.selected_item + 1 - height.max(1);
            }
            let has_field = items.iter().any(|item| item.field.is_some());
            let has_score = items.iter().any(|item| item.score.is_some());
            let field_width = if has_field { body.width / 3 } else { 0 };
            for (offset, item) in items.iter().enumerate().skip(tab.item_scroll).take(height) {
                let y = body.y + (offset - tab.item_scroll) as u16;
                let selected = offset == tab.selected_item;
                let style = match (selected, value_focused) {
                    (true, true) => styles.selected(),
                    (true, false) => Style::default()
                        .fg(styles.foreground)
                        .bg(styles.cursor_line),
                    _ => Style::default().fg(styles.foreground),
                };
                buffer.set_style(Rect::new(body.x, y, body.width, 1), style);
                let mut x = body.x + 1;
                let prefix = match (item.index, item.score) {
                    (_, Some(score)) if has_score => format!("{score:>10} "),
                    (Some(index), _) => format!("{index:>5} "),
                    _ => String::new(),
                };
                x = put(
                    buffer,
                    x,
                    y,
                    &prefix,
                    body.width,
                    style.fg(styles.syntax_number),
                );
                if let Some(field) = &item.field {
                    put(
                        buffer,
                        x,
                        y,
                        &text::fit(&String::from_utf8_lossy(field), usize::from(field_width)),
                        field_width,
                        style.fg(styles.accent),
                    );
                    x += field_width + 1;
                }
                let value = String::from_utf8_lossy(&item.value).replace(['\n', '\r'], " ");
                put(
                    buffer,
                    x,
                    y,
                    &value,
                    (body.x + body.width).saturating_sub(x + 1),
                    style,
                );
            }
            let footer = format!(
                " {} element(s){} ",
                items.len(),
                if next_cursor.is_some() {
                    " · m: load more"
                } else {
                    ""
                }
            );
            let y = body.y + body.height.saturating_sub(1);
            put(buffer, body.x, y, &footer, body.width, styles.muted());
        }
        Some(ValueContent::Unsupported(kind)) => {
            put(
                buffer,
                body.x + 1,
                body.y + 1,
                &format!("{kind}: use the console to inspect this value"),
                body.width,
                styles.muted(),
            );
        }
        None => {
            put(
                buffer,
                body.x + 1,
                body.y + 1,
                &format!("{} loading…", ui.spinner()),
                body.width,
                styles.muted(),
            );
        }
    }
}

fn serde_pretty(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    relix_query::export::pretty_json(text)
}

fn draw_console(
    frame: &mut Frame<'_>,
    area: Rect,
    tab: &KeyValueTab,
    focused: bool,
    ui: &UiState,
) -> Option<Position> {
    let styles = &ui.styles;
    let console_focused = focused && tab.focus == KeyValueFocus::Console;
    let buffer = frame.buffer_mut();
    let header_style = Style::default()
        .fg(if console_focused {
            styles.accent
        } else {
            styles.muted
        })
        .bg(styles.surface);
    buffer.set_style(Rect::new(area.x, area.y, area.width, 1), header_style);
    put(
        buffer,
        area.x,
        area.y,
        " CONSOLE  (: to focus · ↑↓ history) ",
        area.width,
        bold(header_style),
    );
    let output_rows = usize::from(area.height.saturating_sub(2));
    let start = tab.console_output.len().saturating_sub(output_rows);
    for (offset, line) in tab.console_output.iter().skip(start).enumerate() {
        let color = if line.is_error {
            styles.error
        } else if line.is_input {
            styles.accent
        } else {
            styles.foreground
        };
        put(
            buffer,
            area.x + 1,
            area.y + 1 + offset as u16,
            &line.text,
            area.width.saturating_sub(2),
            Style::default().fg(color),
        );
    }
    let y = area.y + area.height.saturating_sub(1);
    put(
        buffer,
        area.x,
        y,
        " ❯ ",
        3,
        Style::default().fg(styles.accent),
    );
    let position = input(
        buffer,
        Rect::new(area.x + 3, y, area.width.saturating_sub(4), 1),
        &tab.console_input,
        Style::default().fg(styles.foreground),
        false,
    );
    console_focused.then_some(position)
}
