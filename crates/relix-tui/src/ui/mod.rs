/*
 * @author t0bx
 * @since 23.10.2026
 */

mod document;
mod editor;
mod grid;
mod header;
mod keyvalue;
mod overlays;
mod panes;
mod sidebar;
mod status;
mod table;
mod toasts;
pub mod widgets;

use crate::colors::{ColorDepth, Styles};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::widgets::Paragraph;
use relix_app::App;
use relix_app::overlay::JumpTarget;
use relix_app::workspace::TabId;
use relix_core::Dialect;
use relix_editor::HighlightSpan;
use std::cell::RefCell;
use std::collections::HashMap;

const MIN_WIDTH: u16 = 50;
const MIN_HEIGHT: u16 = 12;
const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub struct UiState {
    pub styles: Styles,
    theme_name: String,
    depth: ColorDepth,
    spinner: usize,
    pub(crate) highlights: RefCell<HashMap<TabId, HighlightCache>>,
    pub(crate) jump_positions: RefCell<Vec<(JumpTarget, Position)>>,
}

pub(crate) struct HighlightCache {
    pub version: u64,
    pub dialect: Dialect,
    pub lines: Vec<Vec<HighlightSpan>>,
}

impl UiState {
    pub fn new(app: &App) -> Self {
        let depth = ColorDepth::detect();
        Self::with_depth(app, depth)
    }

    pub fn with_depth(app: &App, depth: ColorDepth) -> Self {
        Self {
            styles: Styles::from_theme(&app.theme, depth),
            theme_name: app.theme.name.clone(),
            depth,
            spinner: 0,
            highlights: RefCell::new(HashMap::new()),
            jump_positions: RefCell::new(Vec::new()),
        }
    }

    pub fn advance_spinner(&mut self) {
        self.spinner = (self.spinner + 1) % SPINNER.len();
    }

    pub fn spinner(&self) -> &'static str {
        SPINNER[self.spinner]
    }

    fn sync_theme(&mut self, app: &App) {
        if self.theme_name != app.theme.name {
            self.styles = Styles::from_theme(&app.theme, self.depth);
            self.theme_name = app.theme.name.clone();
        }
    }
}

pub fn draw(frame: &mut Frame<'_>, app: &mut App, ui: &mut UiState) {
    ui.sync_theme(app);
    app.hit_map.clear();
    ui.jump_positions.borrow_mut().clear();
    let area = frame.area();
    frame.buffer_mut().set_style(area, ui.styles.base());
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let message = format!(
            "RELIX needs at least {MIN_WIDTH}×{MIN_HEIGHT} (now {}×{})",
            area.width, area.height
        );
        frame.render_widget(
            Paragraph::new(message).style(Style::default().fg(ui.styles.warning)),
            area,
        );
        return;
    }
    let show_hints = app.settings.ui.show_hints;
    let [header_area, body, hints_area, status_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(u16::from(show_hints)),
        Constraint::Length(1),
    ])
    .areas(area);
    header::draw(frame, header_area, app, ui);
    let (sidebar_area, workspace_area) = split_body(body, app);
    if let Some(sidebar_area) = sidebar_area {
        sidebar::draw(frame, sidebar_area, app, ui);
    }
    panes::draw(frame, workspace_area, app, ui);
    if show_hints {
        status::draw_hints(frame, hints_area, app, ui);
    }
    status::draw(frame, status_area, app, ui);
    overlays::draw(frame, area, app, ui);
    toasts::draw(frame, body, app, ui);
}

fn split_body(body: Rect, app: &App) -> (Option<Rect>, Rect) {
    if !app.sidebar_visible {
        return (None, body);
    }
    let width = app.sidebar_width.min(body.width / 2).max(16);
    let [sidebar, workspace] =
        Layout::horizontal([Constraint::Length(width), Constraint::Min(10)]).areas(body);
    (Some(sidebar), workspace)
}
