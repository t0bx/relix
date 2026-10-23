/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::Notify;

/// Lets `cancel` stop the console statement that is currently running. DynamoDB has no
/// server-side cancellation, so aborting stops waiting for the current request and
/// fetching further pages; a write the service already accepted still completes.
#[derive(Default)]
pub struct AbortSignal {
    running: Mutex<Option<Arc<Notify>>>,
}

impl AbortSignal {
    pub fn begin(&self) -> Arc<Notify> {
        let notify = Arc::new(Notify::new());
        *self.running.lock().unwrap_or_else(PoisonError::into_inner) = Some(notify.clone());
        notify
    }

    pub fn finish(&self) {
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }

    pub fn abort(&self) -> bool {
        let running = self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        match running {
            Some(notify) => {
                notify.notify_one();
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn abort_wakes_the_running_statement() {
        let signal = AbortSignal::default();
        assert!(!signal.abort());
        let notified = signal.begin();
        assert!(signal.abort());
        tokio::time::timeout(Duration::from_secs(1), notified.notified())
            .await
            .unwrap();
        signal.finish();
        assert!(!signal.abort());
    }
}
