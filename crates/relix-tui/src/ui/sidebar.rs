/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::UiState;
use super::widgets::{fill_line, input, panel, put, title_line, to_area};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use relix_app::app::ConnectionStatus;
use relix_app::explorer::{Children, ExplorerNode, NodeKind};
use relix_app::{App, Focus, HitTarget};
use relix_core::ObjectKind;

pub fn draw(frame: &mut Frame<'_>, area: Rect, app: &mut App, ui: &UiState) {
    let styles = &ui.styles;
    let focused = app.focus == Focus::Sidebar;
    let block = panel(styles, title_line(styles, "DATA SOURCES", focused), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let show_filter = app.explorer.filtering || !app.explorer.filter.is_empty();
    let list_height = inner.height.saturating_sub(u16::from(show_filter));
    let visible = app.explorer.visible();
    if visible.is_empty() {
        let hint = format!(
            "{} adds a data source",
            app.keymap
                .primary_key(relix_app::command::Command::AddDataSource)
                .unwrap_or_else(|| "Space a".into())
        );
        put(
            frame.buffer_mut(),
            inner.x + 1,
            inner.y,
            &hint,
            inner.width.saturating_sub(1),
            styles.muted(),
        );
    }
    let height = usize::from(list_height.max(1));
    if app.explorer.selected < app.explorer.scroll {
        app.explorer.scroll = app.explorer.selected;
    } else if app.explorer.selected >= app.explorer.scroll + height {
        app.explorer.scroll = app.explorer.selected + 1 - height;
    }
    for (row, id) in visible
        .iter()
        .enumerate()
        .skip(app.explorer.scroll)
        .take(height)
    {
        let y = inner.y + (row - app.explorer.scroll) as u16;
        let Some(node) = app.explorer.node(*id) else {
            continue;
        };
        let selected = row == app.explorer.selected;
        if selected {
            let style = if focused {
                styles.selected()
            } else {
                Style::default().bg(styles.cursor_line)
            };
            fill_line(frame.buffer_mut(), inner, y, style);
        }
        let expanded = app.explorer.is_expanded(*id);
        draw_node(
            frame,
            inner,
            y,
            node,
            expanded,
            selected && focused,
            app,
            ui,
        );
        app.hit_map.push(
            to_area(Rect::new(inner.x, y, inner.width, 1)),
            HitTarget::ExplorerRow(row),
        );
    }
    if show_filter {
        let y = inner.y + list_height;
        let buffer = frame.buffer_mut();
        put(
            buffer,
            inner.x,
            y,
            "/",
            1,
            Style::default().fg(styles.accent),
        );
        let position = input(
            buffer,
            Rect::new(inner.x + 2, y, inner.width.saturating_sub(2), 1),
            &app.explorer.filter,
            styles.base(),
            false,
        );
        if app.explorer.filtering && focused && app.overlays.is_empty() {
            frame.set_cursor_position(position);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_node(
    frame: &mut Frame<'_>,
    area: Rect,
    y: u16,
    node: &ExplorerNode,
    expanded: bool,
    highlighted: bool,
    app: &App,
    ui: &UiState,
) {
    let styles = &ui.styles;
    let indent = node.depth * 2;
    let mut x = area.x + 1 + indent;
    let right = area.x + area.width;
    let marker = match (&node.children, expanded) {
        _ if node.kind.is_leaf() => " ",
        (Children::Loading, _) => ui.spinner(),
        (Children::Failed(_), _) => "✗",
        (_, true) => "▾",
        (_, false) => "▸",
    };
    let marker_color = if matches!(node.children, Children::Failed(_)) {
        styles.error
    } else {
        styles.muted
    };
    let buffer = frame.buffer_mut();
    x = put(
        buffer,
        x,
        y,
        marker,
        right.saturating_sub(x),
        Style::default().fg(marker_color),
    );
    x = put(buffer, x, y, " ", right.saturating_sub(x), Style::default());
    let (icon, icon_color) = icon(&node.kind, app, ui);
    x = put(
        buffer,
        x,
        y,
        icon,
        right.saturating_sub(x),
        Style::default().fg(icon_color),
    );
    x = put(buffer, x, y, " ", right.saturating_sub(x), Style::default());
    let mut label_style = Style::default().fg(styles.foreground);
    if highlighted {
        label_style = styles.selected();
    }
    if let NodeKind::DataSource(source) = &node.kind {
        label_style = label_style.add_modifier(Modifier::BOLD);
        if let Some(color) = app
            .data_source(*source)
            .and_then(|definition| styles.environment(definition.environment))
        {
            label_style = label_style.fg(color);
        }
    }
    x = put(
        buffer,
        x,
        y,
        &node.label,
        right.saturating_sub(x),
        label_style,
    );
    let detail = match &node.children {
        Children::Failed(message) => Some((message.clone(), styles.error)),
        _ => node.detail.clone().map(|detail| (detail, styles.muted)),
    };
    if let Some((detail, color)) = detail {
        let x = x + 1;
        put(
            buffer,
            x,
            y,
            &detail,
            right.saturating_sub(x + 1),
            Style::default().fg(color).add_modifier(Modifier::DIM),
        );
    }
}

fn icon(kind: &NodeKind, app: &App, ui: &UiState) -> (&'static str, ratatui::style::Color) {
    let styles = &ui.styles;
    let unicode = app.settings.ui.unicode_icons;
    let pick = |fancy: &'static str, plain: &'static str| if unicode { fancy } else { plain };
    match kind {
        NodeKind::DataSource(source) => {
            let color = match app.connection_status(*source) {
                ConnectionStatus::Connected => styles.success,
                ConnectionStatus::Connecting => styles.warning,
                ConnectionStatus::Failed(_) => styles.error,
                ConnectionStatus::Disconnected => styles.muted,
            };
            (pick("●", "*"), color)
        }
        NodeKind::Database { .. } => (pick("◫", "D"), styles.accent),
        NodeKind::Schema { .. } => (pick("◇", "S"), styles.accent_secondary),
        NodeKind::Folder { .. } | NodeKind::Detail { .. } => (pick("▤", "+"), styles.muted),
        NodeKind::Object { info, .. } => match info.kind {
            ObjectKind::Table => (pick("▦", "T"), styles.accent),
            ObjectKind::View | ObjectKind::MaterializedView => (pick("◨", "V"), styles.info),
            ObjectKind::Function | ObjectKind::Procedure => {
                (pick("ƒ", "f"), styles.syntax_function)
            }
            ObjectKind::Sequence => (pick("#", "#"), styles.syntax_number),
            ObjectKind::Trigger => (pick("↻", "!"), styles.warning),
        },
        NodeKind::Column(column) if column.is_primary_key => (pick("⚷", "k"), styles.warning),
        NodeKind::Column(_) => (pick("◦", "-"), styles.muted),
        NodeKind::Index(_) => (pick("↯", "i"), styles.muted),
        NodeKind::ForeignKey(_) => (pick("⇢", ">"), styles.info),
        NodeKind::KeySpace(_) => (pick("⚿", "K"), styles.accent),
        NodeKind::Message(_) => ("!", styles.error),
    }
}
