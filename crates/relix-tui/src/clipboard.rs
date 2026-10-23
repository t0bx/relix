/*
 * @author t0bx
 * @since 23.10.2026
 */

use crossterm::clipboard::CopyToClipboard;
use crossterm::execute;
use relix_app::services::Clipboard;

/// Copies through the terminal's OSC 52 escape sequence, which also works over SSH and
/// needs no display server. Paste uses the terminal's own (bracketed) paste; this type
/// additionally keeps the last copied text for in-app paste.
#[derive(Debug, Default)]
pub struct Osc52Clipboard {
    last: Option<String>,
}

impl Clipboard for Osc52Clipboard {
    fn set_text(&mut self, text: &str) -> Result<(), String> {
        self.last = Some(text.to_owned());
        execute!(std::io::stdout(), CopyToClipboard::to_clipboard_from(text))
            .map_err(|error| error.to_string())
    }

    fn text(&mut self) -> Option<String> {
        self.last.clone()
    }
}
