/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};
use std::future::Future;
use tokio::sync::Notify;

#[derive(Default)]
pub(crate) struct CancelSignal {
    notify: Notify,
}

impl CancelSignal {
    /// Runs the operation until it finishes or `cancel` is called. CQL has no request
    /// cancellation in the native protocol, so cancelling stops waiting for the
    /// response and frees the session; the coordinator finishes (or times out) the
    /// request on its own.
    pub(crate) async fn run<T>(
        &self,
        operation: impl Future<Output = Result<T, DriverError>>,
    ) -> Result<T, DriverError> {
        let cancelled = self.notify.notified();
        tokio::select! {
            result = operation => result,
            _ = cancelled => Err(DriverError::new(
                ErrorKind::Cancelled,
                "statement cancelled; the server may still complete it",
            )),
        }
    }

    pub(crate) fn cancel(&self) {
        self.notify.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn cancel_interrupts_running_operations() {
        let signal = Arc::new(CancelSignal::default());
        let running = {
            let signal = signal.clone();
            tokio::spawn(async move {
                signal
                    .run(async {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        Ok(())
                    })
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        signal.cancel();
        let error = running.await.unwrap().unwrap_err();
        assert_eq!(error.kind, ErrorKind::Cancelled);
    }

    #[tokio::test]
    async fn idle_cancel_does_not_affect_later_operations() {
        let signal = CancelSignal::default();
        signal.cancel();
        assert_eq!(signal.run(async { Ok(5) }).await.unwrap(), 5);
    }
}
