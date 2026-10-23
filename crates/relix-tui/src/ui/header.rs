/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::UiState;
use super::widgets::bold;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use relix_app::App;
use relix_app::app::ConnectionStatus;
use relix_app::tab::Tab;

pub fn draw(frame: &mut Frame<'_>, area: Rect, app: &App, ui: &UiState) {
    let styles = &ui.styles;
    let separator = || Span::styled(" │ ", Style::default().fg(styles.border));
    let mut spans = vec![
        Span::styled(
            " RELIX ",
            bold(Style::default().fg(styles.background).bg(styles.accent)),
        ),
        separator(),
        Span::styled("Project: ", styles.muted()),
        Span::styled(
            app.project_name().unwrap_or("—").to_owned(),
            bold(Style::default().fg(styles.foreground)),
        ),
    ];
    let source = app.active_tab().and_then(Tab::data_source).or_else(|| {
        app.current_project()
            .and_then(|project| project.settings.default_data_source)
    });
    if let Some(definition) = source.and_then(|source| app.data_source(source)) {
        spans.push(separator());
        spans.push(Span::styled(
            format!("{}: ", app.driver_name(&definition.driver)),
            styles.muted(),
        ));
        spans.push(Span::styled(
            definition.name.clone(),
            Style::default().fg(styles.foreground),
        ));
        if let Some(color) = styles.environment(definition.environment) {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!(" {} ", definition.environment.label()),
                bold(Style::default().fg(styles.background).bg(color)),
            ));
        }
        spans.push(separator());
        let (dot, label, color) = match app.connection_status(definition.id) {
            ConnectionStatus::Connected => ("●", "Connected", styles.success),
            ConnectionStatus::Connecting => (ui.spinner(), "Connecting", styles.warning),
            ConnectionStatus::Failed(_) => ("●", "Failed", styles.error),
            ConnectionStatus::Disconnected => ("○", "Disconnected", styles.muted),
        };
        spans.push(Span::styled(
            format!("{dot} {label}"),
            Style::default().fg(color),
        ));
        if let Some(Tab::Editor(editor)) = app.active_tab()
            && editor.in_transaction
        {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                " TX ",
                bold(Style::default().fg(styles.background).bg(styles.warning)),
            ));
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(styles.status_bg)),
        area,
    );
}
