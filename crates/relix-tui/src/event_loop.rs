/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::input_map::map_event;
use crate::terminal::TerminalGuard;
use crate::ui::{UiState, draw};
use crossterm::event::EventStream;
use futures_util::StreamExt;
use relix_app::{App, AppEvent, InputEvent};
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

const TICK: Duration = Duration::from_millis(250);
const MAX_EVENTS_PER_FRAME: usize = 256;

/// Drives the application: terminal input, results of background tasks and a periodic
/// tick are multiplexed without ever blocking on database work. The UI is redrawn only
/// after something changed, and bursts of task results are coalesced into one frame.
pub async fn run(
    app: &mut App,
    mut events: mpsc::Receiver<AppEvent>,
    guard: &mut TerminalGuard,
) -> io::Result<()> {
    let mut input = EventStream::new();
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut ui = UiState::new(app);
    let mut dirty = true;
    loop {
        if dirty {
            guard.terminal.draw(|frame| draw(frame, app, &mut ui))?;
            dirty = false;
        }
        if app.should_quit {
            break;
        }
        tokio::select! {
            next = input.next() => match next {
                Some(Ok(event)) => {
                    if let Some(input) = map_event(event) {
                        app.handle_input(input);
                        dirty = true;
                    }
                }
                Some(Err(error)) => return Err(error),
                None => break,
            },
            Some(event) = events.recv() => {
                app.handle_event(event);
                for _ in 0..MAX_EVENTS_PER_FRAME {
                    match events.try_recv() {
                        Ok(event) => app.handle_event(event),
                        Err(_) => break,
                    }
                }
                dirty = true;
            }
            _ = ticker.tick() => {
                app.handle_input(InputEvent::Tick);
                ui.advance_spinner();
                dirty = true;
            }
        }
    }
    app.shutdown();
    Ok(())
}
