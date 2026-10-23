/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::UiState;
use super::widgets::{bold, panel, put, to_area};
use super::{document, editor, keyvalue, table};
use crate::text;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use relix_app::command::Command;
use relix_app::tab::Tab;
use relix_app::workspace::{Area, PaneId};
use relix_app::{App, Focus, HitTarget};

pub fn draw(frame: &mut Frame<'_>, area: Rect, app: &mut App, ui: &UiState) {
    let areas = app
        .workspace
        .layout()
        .areas(Area::new(area.x, area.y, area.width, area.height));
    for (pane, pane_area) in areas {
        let rect = Rect::new(pane_area.x, pane_area.y, pane_area.width, pane_area.height);
        draw_pane(frame, rect, pane, app, ui);
    }
}

fn draw_pane(frame: &mut Frame<'_>, area: Rect, pane: PaneId, app: &mut App, ui: &UiState) {
    let styles = &ui.styles;
    let focused = app.focus == Focus::Workspace && app.workspace.focused_pane() == pane;
    let active = app.workspace.pane(pane).and_then(|pane| pane.active_tab());
    let title = active
        .and_then(|tab| app.workspace.tab(tab))
        .map(|tab| pane_title(app, tab))
        .unwrap_or_default();
    let block = panel(
        styles,
        Line::from(Span::styled(format!(" {title} "), styles.title(focused))),
        focused,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    app.hit_map.push(to_area(area), HitTarget::Pane(pane));
    if inner.height < 2 || inner.width < 4 {
        return;
    }
    let tabs_area = Rect::new(inner.x, inner.y, inner.width, 1);
    draw_tab_bar(frame, tabs_area, pane, app, ui, focused);
    let content = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    let Some(tab_id) = active else {
        draw_welcome(frame, content, app, ui);
        return;
    };
    match app.workspace.tab(tab_id) {
        Some(Tab::Editor(_)) => editor::draw(frame, content, tab_id, pane, focused, app, ui),
        Some(Tab::Table(_)) => table::draw(frame, content, tab_id, pane, focused, app, ui),
        Some(Tab::KeyValue(_)) => keyvalue::draw(frame, content, tab_id, pane, focused, app, ui),
        Some(Tab::Document(_)) => document::draw(frame, content, tab_id, focused, app, ui),
        None => {}
    }
}

fn pane_title(app: &App, tab: &Tab) -> String {
    let source = tab
        .data_source()
        .and_then(|source| app.data_source(source))
        .map(|definition| definition.name.clone());
    match (tab, source) {
        (Tab::Table(table), Some(source)) => format!("{} — {source}", table.table),
        (_, Some(source)) => format!("{} — {source}", tab.title()),
        (_, None) => tab.title(),
    }
}

fn draw_tab_bar(
    frame: &mut Frame<'_>,
    area: Rect,
    pane: PaneId,
    app: &mut App,
    ui: &UiState,
    focused: bool,
) {
    let styles = &ui.styles;
    let Some(pane_state) = app.workspace.pane(pane).cloned() else {
        return;
    };
    let mut x = area.x;
    let right = area.x + area.width;
    for (index, tab_id) in pane_state.tabs.iter().enumerate() {
        let Some(tab) = app.workspace.tab(*tab_id) else {
            continue;
        };
        let icon = match tab {
            Tab::Editor(_) => "≡",
            Tab::Table(_) => "▦",
            Tab::KeyValue(_) => "⚿",
            Tab::Document(_) => "❏",
        };
        let label = format!(" {icon} {} ", text::truncate(&tab.title(), 24));
        let is_active = index == pane_state.active;
        let style = match (is_active, focused) {
            (true, true) => bold(Style::default().fg(styles.background).bg(styles.accent)),
            (true, false) => bold(
                Style::default()
                    .fg(styles.foreground)
                    .bg(styles.cursor_line),
            ),
            _ => Style::default().fg(styles.muted),
        };
        let start = x;
        x = put(
            frame.buffer_mut(),
            x,
            area.y,
            &label,
            right.saturating_sub(x),
            style,
        );
        app.hit_map.push(
            Area::new(start, area.y, x - start, 1),
            HitTarget::TabHeader(*tab_id),
        );
        if x >= right {
            break;
        }
    }
}

fn draw_welcome(frame: &mut Frame<'_>, area: Rect, app: &App, ui: &UiState) {
    let styles = &ui.styles;
    let lines = [
        (Command::NewEditor, "New query tab"),
        (Command::AddDataSource, "Add data source"),
        (Command::OpenProjects, "Projects"),
        (Command::OpenPalette, "Command palette"),
        (Command::FocusExplorer, "Database explorer"),
        (Command::ShowHelp, "Help"),
    ];
    let top = area.y + area.height.saturating_sub(lines.len() as u16 + 2) / 2;
    let buffer = frame.buffer_mut();
    let heading = "RELIX — a database IDE for your terminal";
    let x = area.x + area.width.saturating_sub(text::width(heading) as u16) / 2;
    put(
        buffer,
        x,
        top,
        heading,
        area.width,
        bold(Style::default().fg(styles.accent)),
    );
    for (row, (command, label)) in lines.iter().enumerate() {
        let key = app.keymap.primary_key(*command).unwrap_or_default();
        let line = format!("{} {}", text::fit_right(&key, 12), label);
        let x = area.x + area.width.saturating_sub(34) / 2;
        put(
            buffer,
            x,
            top + 2 + row as u16,
            &line,
            area.width,
            styles.muted(),
        );
    }
}
