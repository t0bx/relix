/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::UiState;
use super::widgets::{bold, input, put};
use crate::text;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use relix_app::HitTarget;
use relix_app::app::HitMap;
use relix_app::grid::{GridState, GridView, RowOrigin, cell_text};
use relix_app::overlay::JumpTarget;
use relix_app::workspace::{Area, PaneId};
use relix_core::{SortDirection, SortKey, Value};

pub struct GridFrame<'a> {
    pub sort: &'a [SortKey],
    pub focused: bool,
    pub pane: PaneId,
    pub row_number_offset: u64,
}

/// Renders a grid and returns the cursor position of an active inline cell editor.
pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    grid: &mut GridState,
    info: &GridFrame<'_>,
    hit_map: &mut HitMap,
    ui: &UiState,
) -> Option<Position> {
    if area.height < 2 || area.width < 4 {
        return None;
    }
    if grid.view == GridView::Json {
        draw_json(frame, area, grid, ui);
        return None;
    }
    let styles = &ui.styles;
    let last_row_number = info.row_number_offset + grid.rows.len() as u64;
    let gutter = (last_row_number.max(1).to_string().len() as u16 + 1).max(3);
    grid.viewport_rows = usize::from(area.height - 1);
    grid.ensure_row_visible();
    let columns = visible_window(grid, area.width.saturating_sub(gutter + 1));
    let buffer = frame.buffer_mut();
    let header_style = bold(
        Style::default()
            .fg(styles.grid_header_fg)
            .bg(styles.grid_header_bg),
    );
    buffer.set_style(Rect::new(area.x, area.y, area.width, 1), header_style);
    put(
        buffer,
        area.x,
        area.y,
        &text::fit_right("#", usize::from(gutter - 1)),
        gutter,
        header_style.fg(styles.muted),
    );
    let mut x = area.x + gutter + 1;
    let right = area.x + area.width;
    let mut cell_positions = Vec::new();
    for &index in &columns {
        let column = &grid.columns[index];
        let width = column.width.min(right.saturating_sub(x));
        let mut label = column.meta.name.clone();
        if column.is_key {
            label = format!("⚷{label}");
        }
        if let Some(position) = info
            .sort
            .iter()
            .position(|key| key.column == column.meta.name)
        {
            let arrow = match info.sort[position].direction {
                SortDirection::Ascending => "▲",
                SortDirection::Descending => "▼",
            };
            let order = if info.sort.len() > 1 {
                (position + 1).to_string()
            } else {
                String::new()
            };
            label = format!("{label} {arrow}{order}");
        }
        let style = if index == grid.cursor_column && info.focused {
            header_style.fg(styles.accent)
        } else {
            header_style
        };
        put(
            buffer,
            x,
            area.y,
            &text::fit(&label, usize::from(width)),
            width,
            style,
        );
        ui.jump_positions
            .borrow_mut()
            .push((JumpTarget::Column(index), Position::new(x, area.y)));
        cell_positions.push((index, x, width));
        x = x.saturating_add(column.width + 1);
        if x >= right {
            break;
        }
    }
    let mut editor_cursor = None;
    let visible_rows = grid
        .rows
        .len()
        .saturating_sub(grid.row_offset)
        .min(grid.viewport_rows);
    for offset in 0..visible_rows {
        let row_index = grid.row_offset + offset;
        let y = area.y + 1 + offset as u16;
        let row = &grid.rows[row_index];
        let selected = grid.selected.contains(&row_index);
        let is_cursor_row = row_index == grid.cursor_row;
        let mut row_style = Style::default().fg(styles.foreground);
        if row_index % 2 == 1 {
            row_style = row_style.bg(styles.grid_row_alt_bg);
        }
        if row.origin == RowOrigin::Inserted {
            row_style = row_style.bg(styles.grid_inserted_bg);
        }
        if selected {
            row_style = row_style.fg(styles.selection_fg).bg(styles.selection_bg);
        } else if is_cursor_row && info.focused {
            row_style = row_style.bg(styles.cursor_line);
        }
        if row.deleted {
            row_style = row_style
                .fg(styles.grid_deleted_fg)
                .add_modifier(Modifier::CROSSED_OUT);
        }
        buffer.set_style(Rect::new(area.x, y, area.width, 1), row_style);
        let marker = match (row.origin, row.deleted, row.edits.is_empty()) {
            (RowOrigin::Inserted, _, _) => "+",
            (_, true, _) => "-",
            (_, _, false) => "*",
            _ => "",
        };
        let number = info.row_number_offset + row_index as u64 + 1;
        let gutter_text = format!("{marker}{number}");
        put(
            buffer,
            area.x,
            y,
            &text::fit_right(&gutter_text, usize::from(gutter - 1)),
            gutter,
            row_style
                .fg(styles.muted)
                .remove_modifier(Modifier::CROSSED_OUT),
        );
        ui.jump_positions
            .borrow_mut()
            .push((JumpTarget::Row(row_index), Position::new(area.x, y)));
        hit_map.push(
            Area::new(area.x, y, area.width, 1),
            HitTarget::GridRow {
                pane: info.pane,
                row: row_index,
            },
        );
        for &(index, cell_x, width) in &cell_positions {
            let value = row.value(index);
            let mut style = row_style;
            if value.is_null() {
                style = style.fg(styles.grid_null).add_modifier(Modifier::ITALIC);
            }
            if row.is_modified(index) {
                style = style.bg(styles.grid_modified_bg);
            }
            let is_cursor = is_cursor_row && index == grid.cursor_column;
            if is_cursor && info.focused {
                style = bold(style.bg(styles.grid_cursor_bg).fg(styles.selection_fg));
            }
            let content = cell_text(value, usize::from(width));
            let content = if value.is_numeric() || numeric_text(value) {
                text::fit_right(&content, usize::from(width))
            } else {
                text::fit(&content, usize::from(width))
            };
            put(buffer, cell_x, y, &content, width, style);
            let editing_here = grid
                .editing
                .as_ref()
                .is_some_and(|editor| editor.row == row_index && editor.column == index);
            if editing_here && let Some(editor) = grid.editing.as_ref() {
                let edit_width = width.max(24).min(right.saturating_sub(cell_x));
                let edit_style = Style::default()
                    .fg(styles.foreground)
                    .bg(styles.surface)
                    .add_modifier(Modifier::UNDERLINED);
                editor_cursor = Some(input(
                    buffer,
                    Rect::new(cell_x, y, edit_width, 1),
                    &editor.input,
                    edit_style,
                    false,
                ));
            }
        }
    }
    if grid.rows.is_empty() {
        put(
            buffer,
            area.x + gutter + 1,
            area.y + 1,
            "(no rows)",
            area.width.saturating_sub(gutter + 1),
            styles.muted(),
        );
    }
    editor_cursor
}

fn numeric_text(value: &Value) -> bool {
    value
        .as_text()
        .is_some_and(|text| !text.is_empty() && text.len() < 32 && text.parse::<f64>().is_ok())
}

/// Picks the columns to show so the cursor column is always visible, scrolling the
/// window horizontally as the cursor moves.
fn visible_window(grid: &mut GridState, width: u16) -> Vec<usize> {
    let visible = grid.visible_columns();
    let Some(cursor) = visible
        .iter()
        .position(|index| *index == grid.cursor_column)
    else {
        return visible;
    };
    let mut start = visible
        .iter()
        .position(|index| *index >= grid.column_offset)
        .unwrap_or(0)
        .min(cursor);
    loop {
        let used: u16 = visible[start..=cursor]
            .iter()
            .map(|index| grid.columns[*index].width + 1)
            .sum();
        if used <= width || start == cursor {
            break;
        }
        start += 1;
    }
    grid.column_offset = visible[start];
    visible[start..].to_vec()
}

fn draw_json(frame: &mut Frame<'_>, area: Rect, grid: &mut GridState, ui: &UiState) {
    let styles = &ui.styles;
    let json = grid.json_text();
    let lines: Vec<&str> = json.lines().collect();
    grid.json_scroll = grid.json_scroll.min(lines.len().saturating_sub(1));
    let buffer = frame.buffer_mut();
    for (offset, line) in lines
        .iter()
        .skip(grid.json_scroll)
        .take(usize::from(area.height))
        .enumerate()
    {
        let y = area.y + offset as u16;
        let mut x = area.x + 1;
        for (fragment, kind) in json_fragments(line) {
            let style = match kind {
                JsonToken::Key => Style::default().fg(styles.accent),
                JsonToken::String => Style::default().fg(styles.syntax_string),
                JsonToken::Number => Style::default().fg(styles.syntax_number),
                JsonToken::Literal => Style::default().fg(styles.syntax_keyword),
                JsonToken::Punctuation => Style::default().fg(styles.muted),
            };
            x = put(
                buffer,
                x,
                y,
                &fragment,
                (area.x + area.width).saturating_sub(x),
                style,
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsonToken {
    Key,
    String,
    Number,
    Literal,
    Punctuation,
}

/// Splits one line of pretty-printed JSON into coloured fragments.
fn json_fragments(line: &str) -> Vec<(String, JsonToken)> {
    let mut fragments = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' {
            let start = index;
            index += 1;
            while index < chars.len() && chars[index] != '"' {
                if chars[index] == '\\' {
                    index += 1;
                }
                index += 1;
            }
            index = (index + 1).min(chars.len());
            let text: String = chars[start..index].iter().collect();
            let is_key = chars[index..].iter().find(|ch| !ch.is_whitespace()) == Some(&':');
            fragments.push((
                text,
                if is_key {
                    JsonToken::Key
                } else {
                    JsonToken::String
                },
            ));
        } else if ch == '-' || ch.is_ascii_digit() {
            let start = index;
            while index < chars.len()
                && (chars[index].is_ascii_digit()
                    || matches!(chars[index], '-' | '+' | '.' | 'e' | 'E'))
            {
                index += 1;
            }
            fragments.push((chars[start..index].iter().collect(), JsonToken::Number));
        } else if ch.is_ascii_alphabetic() {
            let start = index;
            while index < chars.len() && chars[index].is_ascii_alphabetic() {
                index += 1;
            }
            fragments.push((chars[start..index].iter().collect(), JsonToken::Literal));
        } else {
            fragments.push((ch.to_string(), JsonToken::Punctuation));
            index += 1;
        }
    }
    fragments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_lines_are_tokenized() {
        let kinds: Vec<JsonToken> = json_fragments("  \"id\": 12, \"name\": \"x\", \"ok\": null")
            .into_iter()
            .filter(|(text, _)| !text.trim().is_empty())
            .map(|(_, kind)| kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                JsonToken::Key,
                JsonToken::Punctuation,
                JsonToken::Number,
                JsonToken::Punctuation,
                JsonToken::Key,
                JsonToken::Punctuation,
                JsonToken::String,
                JsonToken::Punctuation,
                JsonToken::Key,
                JsonToken::Punctuation,
                JsonToken::Literal,
            ]
        );
    }
}
