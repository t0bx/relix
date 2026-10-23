/*
 * @author t0bx
 * @since 23.10.2026
 */

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};

static ENHANCED_KEYBOARD: AtomicBool = AtomicBool::new(false);
static MOUSE_CAPTURE: AtomicBool = AtomicBool::new(false);

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Owns the terminal in raw/alternate-screen mode and restores it on drop, so the user's
/// shell is usable again even when the application exits with an error.
pub struct TerminalGuard {
    pub terminal: Tui,
}

impl TerminalGuard {
    pub fn enter(mouse: bool) -> io::Result<Self> {
        install_panic_hook();
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
        if mouse {
            execute!(stdout, EnableMouseCapture)?;
            MOUSE_CAPTURE.store(true, Ordering::Relaxed);
        }
        if supports_keyboard_enhancement().unwrap_or(false) {
            let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS;
            if execute!(stdout, PushKeyboardEnhancementFlags(flags)).is_ok() {
                ENHANCED_KEYBOARD.store(true, Ordering::Relaxed);
            }
        }
        let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        terminal.clear()?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
        let _ = self.terminal.show_cursor();
    }
}

pub fn restore() {
    let mut stdout = io::stdout();
    if ENHANCED_KEYBOARD.swap(false, Ordering::Relaxed) {
        let _ = execute!(stdout, PopKeyboardEnhancementFlags);
    }
    if MOUSE_CAPTURE.swap(false, Ordering::Relaxed) {
        let _ = execute!(stdout, DisableMouseCapture);
    }
    let _ = execute!(stdout, DisableBracketedPaste, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
}
