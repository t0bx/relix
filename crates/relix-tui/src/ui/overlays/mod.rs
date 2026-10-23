/*
 * @author t0bx
 * @since 23.10.2026
 */

mod dialogs;
mod form;
mod help;
mod jump;
mod lists;
mod picker;

use super::UiState;
use ratatui::Frame;
use ratatui::layout::Rect;
use relix_app::App;
use relix_app::overlay::Overlay;

pub fn draw(frame: &mut Frame<'_>, area: Rect, app: &App, ui: &UiState) {
    for overlay in &app.overlays {
        match overlay {
            Overlay::Picker(picker) => picker::draw(frame, area, picker, ui),
            Overlay::DataSourceForm(form) => form::draw(frame, area, form, ui),
            Overlay::Confirm(confirm) => dialogs::confirm(frame, area, confirm, ui),
            Overlay::Prompt(prompt) => dialogs::prompt(frame, area, prompt, ui),
            Overlay::Viewer(viewer) => dialogs::viewer(frame, area, viewer, ui),
            Overlay::TextEdit(edit) => dialogs::text_edit(frame, area, edit, ui),
            Overlay::SortEditor(editor) => lists::sort_editor(frame, area, editor, app, ui),
            Overlay::ColumnChooser(chooser) => lists::column_chooser(frame, area, chooser, app, ui),
            Overlay::Jump(jump) => jump::draw(frame, jump, ui),
            Overlay::Help { scroll } => help::draw(frame, area, *scroll, app, ui),
        }
    }
    if !app.pending_keys.is_empty() && app.overlays.is_empty() {
        help::which_key(frame, area, app, ui);
    }
}
