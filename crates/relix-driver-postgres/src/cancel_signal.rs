/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};
use std::sync::atomic::{AtomicBool, Ordering};

/// Remembers cancel requests for the statement running on the session. Servers speaking
/// the PostgreSQL protocol drop a cancel request that arrives before the statement has
/// started (for example while a slow engine is still preparing it), so the executor
/// checks this signal before it sends the statement itself.
#[derive(Debug, Default)]
pub(crate) struct CancelSignal {
    running: AtomicBool,
    requested: AtomicBool,
}

pub(crate) struct RunningStatement<'a> {
    signal: &'a CancelSignal,
}

impl CancelSignal {
    pub(crate) fn start(&self) -> RunningStatement<'_> {
        self.requested.store(false, Ordering::SeqCst);
        self.running.store(true, Ordering::SeqCst);
        RunningStatement { signal: self }
    }

    pub(crate) fn request(&self) {
        if self.running.load(Ordering::SeqCst) {
            self.requested.store(true, Ordering::SeqCst);
        }
    }

    pub(crate) fn ensure_not_requested(&self) -> Result<(), DriverError> {
        if self.requested.load(Ordering::SeqCst) {
            Err(DriverError::new(
                ErrorKind::Cancelled,
                "canceling statement due to user request",
            ))
        } else {
            Ok(())
        }
    }
}

impl Drop for RunningStatement<'_> {
    fn drop(&mut self) {
        self.signal.running.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_only_apply_to_the_running_statement() {
        let signal = CancelSignal::default();
        signal.request();
        let running = signal.start();
        assert!(signal.ensure_not_requested().is_ok());
        signal.request();
        assert_eq!(
            signal.ensure_not_requested().unwrap_err().kind,
            ErrorKind::Cancelled
        );
        drop(running);
        let _next = signal.start();
        assert!(signal.ensure_not_requested().is_ok());
    }
}
