/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::UiState;
use super::widgets::{bold, key_hint};
use crate::text;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use relix_app::command::Command;
use relix_app::document_tab::DocumentFocus;
use relix_app::editor_tab::{EditorFocus, ResultKind};
use relix_app::keyvalue_tab::KeyValueFocus;
use relix_app::notifications::Level;
use relix_app::tab::Tab;
use relix_app::table_tab::TableFocus;
use relix_app::{App, Focus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Explorer,
    Insert,
    Results,
    Grid,
    EditCell,
    Filter,
    Keys,
    Console,
    Search,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Explorer => "EXPLORER",
            Self::Insert => "INSERT",
            Self::Results => "RESULTS",
            Self::Grid => "GRID",
            Self::EditCell => "EDIT",
            Self::Filter => "FILTER",
            Self::Keys => "KEYS",
            Self::Console => "CONSOLE",
            Self::Search => "SEARCH",
        }
    }
}

fn mode(app: &App) -> Mode {
    if app.focus == Focus::Sidebar {
        return if app.explorer.filtering {
            Mode::Filter
        } else {
            Mode::Explorer
        };
    }
    match app.active_tab() {
        Some(Tab::Editor(editor)) if editor.focus == EditorFocus::Results => Mode::Results,
        Some(Tab::Editor(editor)) if editor.search.is_some() => Mode::Search,
        Some(Tab::Editor(_)) => Mode::Insert,
        Some(Tab::Table(table)) if table.focus == TableFocus::Filter => Mode::Filter,
        Some(Tab::Table(table))
            if table
                .grid
                .as_ref()
                .is_some_and(|grid| grid.editing.is_some()) =>
        {
            Mode::EditCell
        }
        Some(Tab::Table(_)) => Mode::Grid,
        Some(Tab::KeyValue(tab)) => match tab.focus {
            KeyValueFocus::Pattern => Mode::Filter,
            KeyValueFocus::Console => Mode::Console,
            _ => Mode::Keys,
        },
        Some(Tab::Document(tab)) if tab.focus == DocumentFocus::Filter => Mode::Filter,
        Some(Tab::Document(_)) => Mode::Keys,
        None => Mode::Normal,
    }
}

pub fn draw(frame: &mut Frame<'_>, area: Rect, app: &App, ui: &UiState) {
    let styles = &ui.styles;
    let mode = mode(app);
    let badge_color = match mode {
        Mode::Insert | Mode::EditCell | Mode::Filter | Mode::Console | Mode::Search => {
            styles.success
        }
        Mode::Results | Mode::Grid | Mode::Keys => styles.accent_secondary,
        _ => styles.accent,
    };
    let mut left = vec![Span::styled(
        format!(" {} ", mode.label()),
        bold(Style::default().fg(styles.background).bg(badge_color)),
    )];
    if !app.pending_keys.is_empty() {
        let pending: Vec<String> = app.pending_keys.iter().map(ToString::to_string).collect();
        left.push(Span::styled(
            format!(" {} … ", pending.join(" ")),
            Style::default().fg(styles.warning),
        ));
    }
    let message = match app.notifications.latest() {
        Some(notification) if notification.level == Level::Error => {
            (notification.message.clone(), styles.error)
        }
        _ => (app.status.clone(), styles.status_fg),
    };
    let mut right_parts = Vec::new();
    match app.active_tab() {
        Some(Tab::Editor(editor)) => {
            if let Some(running) = &editor.running {
                right_parts.push(format!(
                    "{} running {}/{} · {:.1}s",
                    ui.spinner(),
                    running.completed,
                    running.total,
                    running.started.elapsed().as_secs_f32()
                ));
            } else if let Some(result) = editor.active_result() {
                let rows = match &result.kind {
                    ResultKind::Rows { grid, query, .. } => format!(
                        "Rows: {}{}",
                        grid.rows.len(),
                        if query.has_more { "+" } else { "" }
                    ),
                    _ => result.summary(),
                };
                right_parts.push(format!("{rows} │ Query: {} ms", result.elapsed.as_millis()));
            }
            let cursor = editor.buffer.cursor();
            right_parts.push(format!("Ln {}, Col {}", cursor.line + 1, cursor.column + 1));
        }
        Some(Tab::Table(table)) => {
            if let Some(grid) = &table.grid {
                right_parts.push(format!("Rows: {}", table.query.page_label(grid.rows.len())));
                let pending = grid.pending_count();
                if pending > 0 {
                    right_parts.push(format!("{pending} pending"));
                }
            }
        }
        Some(Tab::KeyValue(tab)) => right_parts.push(format!(
            "Keys: {}{}",
            tab.keys.len(),
            if tab.scan_complete { "" } else { "+" }
        )),
        Some(Tab::Document(tab)) => right_parts.push(tab.page_label()),
        None => {}
    }
    right_parts.push(format!(
        "{} Commands │ {} Help ",
        key_or(app, Command::OpenPalette, "Ctrl+P"),
        key_or(app, Command::ShowHelp, "F1")
    ));
    let right = format!(" {}", right_parts.join(" │ "));
    let left_width: usize = left.iter().map(|span| text::width(&span.content)).sum();
    let available = usize::from(area.width).saturating_sub(left_width + text::width(&right) + 1);
    left.push(Span::styled(
        format!(" {}", text::fit(&message.0, available)),
        Style::default().fg(message.1),
    ));
    left.push(Span::styled(right, Style::default().fg(styles.status_fg)));
    frame.render_widget(
        Paragraph::new(Line::from(left)).style(Style::default().bg(styles.status_bg)),
        area,
    );
}

fn key_or(app: &App, command: Command, fallback: &str) -> String {
    app.keymap
        .primary_key(command)
        .unwrap_or_else(|| fallback.to_owned())
}

/// Context-sensitive shortcut hints (similar to which-key) for the focused region.
pub fn draw_hints(frame: &mut Frame<'_>, area: Rect, app: &App, ui: &UiState) {
    let contexts = app.key_contexts();
    let commands: &[(Command, &str)] = match mode(app) {
        Mode::Explorer => &[
            (Command::Activate, "open"),
            (Command::MoveRight, "expand"),
            (Command::ExplorerNewQuery, "query"),
            (Command::ToggleConnection, "connect"),
            (Command::InspectObject, "inspect"),
            (Command::Filter, "filter"),
            (Command::EditDataSource, "edit"),
            (Command::AddDataSource, "add source"),
        ],
        Mode::Insert => &[
            (Command::RunStatement, "run"),
            (Command::RunScript, "run script"),
            (Command::Complete, "complete"),
            (Command::ChooseDataSource, "data source"),
            (Command::Find, "find"),
            (Command::Save, "save"),
            (Command::ShowHistory, "history"),
            (Command::FocusResults, "results"),
        ],
        Mode::Grid => &[
            (Command::EditCell, "edit"),
            (Command::InsertRow, "insert"),
            (Command::DeleteRows, "delete"),
            (Command::Filter, "where"),
            (Command::SortColumn, "sort"),
            (Command::AddSortColumn, "multi-sort"),
            (Command::ToggleJsonView, "json"),
            (Command::ReviewChanges, "commit"),
            (Command::NextPage, "next page"),
            (Command::JumpRow, "jump"),
        ],
        Mode::Results => &[
            (Command::FocusEditor, "editor"),
            (Command::SortColumn, "sort"),
            (Command::Filter, "where"),
            (Command::ToggleJsonView, "json"),
            (Command::InspectCell, "inspect"),
            (Command::CopyRows, "copy"),
            (Command::ExportResults, "export"),
            (Command::NextPage, "next page"),
        ],
        Mode::Keys => &[
            (Command::Activate, "open"),
            (Command::Filter, "match"),
            (Command::CreateKey, "add"),
            (Command::EditValue, "edit"),
            (Command::DeleteKey, "delete"),
            (Command::SetTtl, "ttl"),
            (Command::LoadMore, "more"),
            (Command::FocusConsole, "console"),
        ],
        _ => &[],
    };
    let styles = &ui.styles;
    let mut spans = Vec::new();
    match mode(app) {
        Mode::Filter | Mode::Console | Mode::EditCell | Mode::Search => {
            spans.extend(key_hint(styles, "Enter", "apply"));
            spans.extend(key_hint(styles, "Esc", "back"));
            if mode(app) == Mode::Filter {
                spans.extend(key_hint(styles, "↑", "history"));
                spans.extend(key_hint(styles, "Ctrl+Space", "complete"));
            }
            if mode(app) == Mode::Search {
                spans.extend(key_hint(styles, "Tab", "replace field"));
                spans.extend(key_hint(styles, "Ctrl+A", "replace all"));
                spans.extend(key_hint(styles, "Alt+C", "case"));
            }
        }
        _ => {
            for (command, label) in commands {
                if let Some(key) = app.keymap.key_in_contexts(&contexts, *command) {
                    spans.extend(key_hint(styles, &key, label));
                }
            }
            spans.extend(key_hint(styles, "Space/Ctrl+K", "more"));
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(styles.background)),
        area,
    );
}
