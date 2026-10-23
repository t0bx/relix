/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::grid::{self, GridFrame};
use super::widgets::{bold, input, put};
use super::{HighlightCache, UiState};
use crate::text;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use relix_app::App;
use relix_app::editor_tab::{EditorFocus, EditorTab, ResultKind, SearchField};
use relix_app::tab::Tab;
use relix_app::workspace::{PaneId, TabId};
use relix_core::Dialect;
use relix_editor::{HighlightClass, Position as TextPosition, highlight_lines};

const COMPLETION_ROWS: usize = 8;

pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    tab_id: TabId,
    pane: PaneId,
    focused: bool,
    app: &mut App,
    ui: &UiState,
) {
    let dialect = match app.workspace.tab(tab_id) {
        Some(Tab::Editor(editor)) => editor
            .data_source
            .map_or(Dialect::Generic, |source| app.dialect(source)),
        _ => return,
    };
    let show_line_numbers = app.settings.ui.show_line_numbers;
    let overlays_open = !app.overlays.is_empty();
    let App {
        workspace, hit_map, ..
    } = app;
    let Some(Tab::Editor(editor)) = workspace.tab_mut(tab_id) else {
        return;
    };
    let (text_area, results_area) = if editor.results.is_empty() {
        (area, None)
    } else {
        let [text, results] =
            Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);
        (text, Some(results))
    };
    let text_focused = focused && editor.focus == EditorFocus::Text && !overlays_open;
    let mut cursor = draw_text(
        frame,
        text_area,
        tab_id,
        editor,
        dialect,
        show_line_numbers,
        text_focused,
        ui,
    );
    if let Some(results_area) = results_area {
        let results_focused = focused && editor.focus == EditorFocus::Results;
        if let Some(position) = draw_results(
            frame,
            results_area,
            editor,
            pane,
            results_focused,
            hit_map,
            ui,
        ) {
            cursor = Some(position);
        }
    }
    if let Some(position) = cursor
        && focused
        && !overlays_open
    {
        frame.set_cursor_position(position);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    frame: &mut Frame<'_>,
    area: Rect,
    tab_id: TabId,
    editor: &mut EditorTab,
    dialect: Dialect,
    show_line_numbers: bool,
    focused: bool,
    ui: &UiState,
) -> Option<Position> {
    let styles = &ui.styles;
    let mut area = area;
    let mut cursor_position = None;
    if let Some(search) = editor.search.as_ref() {
        let rows = if search.replace_mode { 2 } else { 1 };
        let bar = Rect::new(area.x, area.y, area.width, rows);
        cursor_position = draw_search_bar(frame, bar, editor, focused, ui);
        area = Rect::new(
            area.x,
            area.y + rows,
            area.width,
            area.height.saturating_sub(rows),
        );
    }
    let line_count = editor.buffer.line_count();
    let gutter = if show_line_numbers {
        line_count.to_string().len() as u16 + 2
    } else {
        1
    };
    let text_width = area.width.saturating_sub(gutter + 1);
    editor.ensure_cursor_visible(usize::from(area.height), usize::from(text_width));
    let mut cache = ui.highlights.borrow_mut();
    let fresh = cache.get(&tab_id).is_some_and(|cached| {
        cached.version == editor.buffer.version() && cached.dialect == dialect
    });
    if !fresh {
        cache.insert(
            tab_id,
            HighlightCache {
                version: editor.buffer.version(),
                dialect,
                lines: highlight_lines(&editor.buffer.text(), dialect),
            },
        );
    }
    let highlights = cache
        .get(&tab_id)
        .map(|cached| cached.lines.as_slice())
        .unwrap_or_default();
    let selection = editor.buffer.selection();
    let cursor = editor.buffer.cursor();
    let buffer = frame.buffer_mut();
    for row in 0..usize::from(area.height) {
        let line_index = editor.scroll_line + row;
        let y = area.y + row as u16;
        if line_index >= line_count {
            break;
        }
        let is_cursor_line = line_index == cursor.line;
        let line_bg = if is_cursor_line && focused {
            styles.cursor_line
        } else {
            styles.background
        };
        buffer.set_style(
            Rect::new(area.x, y, area.width, 1),
            Style::default().bg(line_bg),
        );
        if show_line_numbers {
            let number_style = if is_cursor_line {
                Style::default().fg(styles.accent)
            } else {
                styles.muted()
            };
            put(
                buffer,
                area.x,
                y,
                &text::fit_right(&(line_index + 1).to_string(), usize::from(gutter - 1)),
                gutter,
                number_style,
            );
        }
        let has_error = editor
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.line == line_index);
        if has_error {
            put(buffer, area.x, y, "●", 1, Style::default().fg(styles.error));
        }
        let line = editor.buffer.line(line_index);
        let spans = highlights
            .get(line_index)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut x = area.x + gutter + 1;
        let right = area.x + area.width;
        for (column, ch) in line.chars().enumerate().skip(editor.scroll_column) {
            let ch = if ch == '\t' { ' ' } else { ch };
            let width = text::width(&ch.to_string()) as u16;
            if x + width > right {
                break;
            }
            let class = spans
                .iter()
                .find(|span| column >= span.start_column && column < span.end_column)
                .map_or(HighlightClass::Plain, |span| span.class);
            let mut style = Style::default().fg(class_color(class, ui)).bg(line_bg);
            if class == HighlightClass::Keyword {
                style = style.add_modifier(Modifier::BOLD);
            }
            if class == HighlightClass::Comment {
                style = style.add_modifier(Modifier::ITALIC);
            }
            let position = TextPosition::new(line_index, column);
            if selection.is_some_and(|(start, end)| position >= start && position < end) {
                style = style.fg(styles.selection_fg).bg(styles.selection_bg);
            }
            let underlined = editor.diagnostics.iter().any(|diagnostic| {
                diagnostic.line == line_index
                    && column >= diagnostic.start_column
                    && column < diagnostic.end_column
            });
            if underlined {
                style = style
                    .add_modifier(Modifier::UNDERLINED)
                    .underline_color(styles.error)
                    .fg(styles.error);
            }
            buffer.set_stringn(x, y, ch.to_string(), usize::from(width), style);
            x += width;
        }
    }
    let line = editor.buffer.line(cursor.line);
    let cursor_x = area.x
        + gutter
        + 1
        + text::width_of_prefix(
            &line.chars().skip(editor.scroll_column).collect::<String>(),
            cursor.column.saturating_sub(editor.scroll_column),
        ) as u16;
    let cursor_y = area.y + (cursor.line.saturating_sub(editor.scroll_line)) as u16;
    let text_cursor = Position::new(
        cursor_x.min(area.x + area.width.saturating_sub(1)),
        cursor_y,
    );
    if focused {
        if let Some(diagnostic) = editor
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.line == cursor.line)
        {
            let message = format!(" {} ", diagnostic.message);
            let y = if cursor_y + 1 < area.y + area.height {
                cursor_y + 1
            } else {
                cursor_y.saturating_sub(1)
            };
            let width = (text::width(&message) as u16).min(area.width.saturating_sub(gutter + 2));
            let x = (area.x + area.width).saturating_sub(width + 1);
            put(
                buffer,
                x,
                y,
                &message,
                width,
                Style::default().fg(styles.background).bg(styles.error),
            );
        }
        if editor.completion.is_some() {
            draw_completion(frame, area, editor, text_cursor, ui);
        }
    }
    if focused && cursor_position.is_none() {
        cursor_position = Some(text_cursor);
    }
    cursor_position
}

fn class_color(class: HighlightClass, ui: &UiState) -> Color {
    let styles = &ui.styles;
    match class {
        HighlightClass::Plain | HighlightClass::Identifier => styles.syntax_identifier,
        HighlightClass::Keyword => styles.syntax_keyword,
        HighlightClass::Function => styles.syntax_function,
        HighlightClass::DataType => styles.syntax_type,
        HighlightClass::String => styles.syntax_string,
        HighlightClass::Number => styles.syntax_number,
        HighlightClass::Comment => styles.syntax_comment,
        HighlightClass::Operator => styles.syntax_operator,
        HighlightClass::Parameter => styles.accent_secondary,
        HighlightClass::Error => styles.error,
    }
}

fn draw_search_bar(
    frame: &mut Frame<'_>,
    area: Rect,
    editor: &EditorTab,
    focused: bool,
    ui: &UiState,
) -> Option<Position> {
    let styles = &ui.styles;
    let search = editor.search.as_ref()?;
    let buffer = frame.buffer_mut();
    let bar_style = Style::default().fg(styles.foreground).bg(styles.surface);
    buffer.set_style(area, bar_style);
    let matches = search.state.matches().len();
    let position = search.state.current_index().map_or(0, |index| index + 1);
    let info = format!(
        " {position}/{matches}{} ",
        if search.state.case_sensitive {
            " Aa"
        } else {
            ""
        }
    );
    let label_width = 9;
    let input_width = area
        .width
        .saturating_sub(label_width + text::width(&info) as u16 + 1);
    put(
        buffer,
        area.x,
        area.y,
        " Find:   ",
        label_width,
        bold(bar_style.fg(styles.accent)),
    );
    let query_cursor = input(
        buffer,
        Rect::new(area.x + label_width, area.y, input_width, 1),
        &search.query,
        bar_style,
        false,
    );
    put(
        buffer,
        area.x + label_width + input_width,
        area.y,
        &info,
        area.width,
        bar_style.fg(styles.muted),
    );
    let mut cursor = (search.field == SearchField::Query).then_some(query_cursor);
    if search.replace_mode && area.height > 1 {
        put(
            buffer,
            area.x,
            area.y + 1,
            " Replace:",
            label_width,
            bold(bar_style.fg(styles.accent)),
        );
        let replace_cursor = input(
            buffer,
            Rect::new(area.x + label_width, area.y + 1, input_width, 1),
            &search.replacement,
            bar_style,
            false,
        );
        if search.field == SearchField::Replacement {
            cursor = Some(replace_cursor);
        }
    }
    cursor.filter(|_| focused)
}

fn draw_completion(
    frame: &mut Frame<'_>,
    area: Rect,
    editor: &EditorTab,
    cursor: Position,
    ui: &UiState,
) {
    let styles = &ui.styles;
    let Some(popup) = editor.completion.as_ref() else {
        return;
    };
    let rows = popup.items.len().min(COMPLETION_ROWS) as u16;
    let width = 48.min(area.width);
    let below = cursor.y + 1 + rows <= area.y + area.height;
    let y = if below {
        cursor.y + 1
    } else {
        cursor.y.saturating_sub(rows)
    };
    let x = cursor.x.min((area.x + area.width).saturating_sub(width));
    let first = popup.selected.saturating_sub(usize::from(rows) - 1);
    let buffer = frame.buffer_mut();
    for (offset, item) in popup
        .items
        .iter()
        .skip(first)
        .take(usize::from(rows))
        .enumerate()
    {
        let selected = first + offset == popup.selected;
        let style = if selected {
            styles.selected()
        } else {
            Style::default().fg(styles.foreground).bg(styles.surface)
        };
        let row_y = y + offset as u16;
        buffer.set_style(Rect::new(x, row_y, width, 1), style);
        let kind = format!(" {:<5}", item.kind.label());
        put(
            buffer,
            x,
            row_y,
            &kind,
            6,
            style.fg(styles.accent_secondary),
        );
        let detail = item.detail.clone().unwrap_or_default();
        let label_width = width.saturating_sub(7 + (text::width(&detail) as u16).min(18));
        put(
            buffer,
            x + 6,
            row_y,
            &format!(" {}", text::fit(&item.label, usize::from(label_width))),
            label_width + 1,
            style,
        );
        put(
            buffer,
            x + 7 + label_width,
            row_y,
            &text::truncate(&detail, 18),
            width.saturating_sub(7 + label_width),
            style.fg(styles.muted),
        );
    }
}

fn draw_results(
    frame: &mut Frame<'_>,
    area: Rect,
    editor: &mut EditorTab,
    pane: PaneId,
    focused: bool,
    hit_map: &mut relix_app::app::HitMap,
    ui: &UiState,
) -> Option<Position> {
    let styles = &ui.styles;
    if area.height < 3 {
        return None;
    }
    let buffer = frame.buffer_mut();
    let header = Rect::new(area.x, area.y, area.width, 1);
    let header_style = Style::default().fg(styles.muted).bg(styles.surface);
    buffer.set_style(header, header_style);
    let mut x = put(
        buffer,
        area.x,
        area.y,
        " RESULTS ",
        9,
        bold(header_style.fg(if focused { styles.accent } else { styles.muted })),
    );
    let right = area.x + area.width;
    for (index, result) in editor.results.iter().enumerate() {
        let ok = !matches!(result.kind, ResultKind::Error(_));
        let marker = if ok { "✓" } else { "✗" };
        let label = format!(" {} {marker} {} ", index + 1, result.summary());
        let mut style = header_style.fg(if ok { styles.success } else { styles.error });
        if index == editor.active_result {
            style = bold(style.bg(styles.cursor_line));
        }
        x = put(buffer, x, area.y, &label, right.saturating_sub(x), style);
    }
    let body = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
    let result = editor.results.get_mut(editor.active_result)?;
    let elapsed = format!(" {} ms ", result.elapsed.as_millis());
    put(
        buffer,
        right.saturating_sub(elapsed.len() as u16),
        area.y,
        &elapsed,
        elapsed.len() as u16,
        header_style,
    );
    match &mut result.kind {
        ResultKind::Rows {
            grid,
            query,
            loading,
            ..
        } => {
            let info = GridFrame {
                sort: &query.sort,
                focused,
                pane,
                row_number_offset: query.offset,
            };
            let cursor = grid::draw(frame, body, grid, &info, hit_map, ui);
            if *loading {
                put(
                    frame.buffer_mut(),
                    body.x + body.width.saturating_sub(12),
                    body.y,
                    &format!(" {} loading ", ui.spinner()),
                    12,
                    Style::default().fg(styles.warning),
                );
            }
            cursor
        }
        ResultKind::Affected(rows) => {
            put(
                buffer,
                body.x + 1,
                body.y + 1,
                &format!("{rows} row(s) affected"),
                body.width,
                Style::default().fg(styles.success),
            );
            put(
                buffer,
                body.x + 1,
                body.y + 2,
                &text::truncate(
                    &result.statement.replace('\n', " "),
                    usize::from(body.width.saturating_sub(2)),
                ),
                body.width,
                styles.muted(),
            );
            None
        }
        ResultKind::Message(message) => {
            put(
                buffer,
                body.x + 1,
                body.y + 1,
                message,
                body.width,
                Style::default().fg(styles.info),
            );
            None
        }
        ResultKind::Error(error) => {
            let mut lines = vec![(
                format!("{}: {}", error.kind.label(), error.message),
                styles.error,
            )];
            if let Some(detail) = &error.detail {
                lines.push((detail.clone(), styles.warning));
            }
            if let Some(position) = error.position {
                lines.push((format!("at character {}", position + 1), styles.muted));
            }
            lines.push((result.statement.replace('\n', " "), styles.muted));
            let mut y = body.y + 1;
            for (line, color) in lines {
                for chunk in wrap(&line, usize::from(body.width.saturating_sub(2))) {
                    if y >= body.y + body.height {
                        break;
                    }
                    put(
                        buffer,
                        body.x + 1,
                        y,
                        &chunk,
                        body.width,
                        Style::default().fg(color),
                    );
                    y += 1;
                }
            }
            None
        }
    }
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(width)
        .map(|chunk| chunk.iter().collect())
        .collect()
}
