/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::DriverError;
use relix_driver_api::ipc::IpcResponse;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use tokio::sync::oneshot;

pub(crate) type Reply = Result<serde_json::Value, DriverError>;

#[derive(Default)]
pub(crate) struct PendingCalls {
    inner: Mutex<PendingState>,
}

#[derive(Default)]
struct PendingState {
    waiting: HashMap<u64, oneshot::Sender<Reply>>,
    failure: Option<DriverError>,
}

impl PendingCalls {
    pub(crate) fn register(&self, id: u64) -> Result<oneshot::Receiver<Reply>, DriverError> {
        let mut state = self.lock();
        if let Some(failure) = &state.failure {
            return Err(failure.clone());
        }
        let (sender, receiver) = oneshot::channel();
        state.waiting.insert(id, sender);
        Ok(receiver)
    }

    pub(crate) fn forget(&self, id: u64) {
        self.lock().waiting.remove(&id);
    }

    pub(crate) fn deliver(&self, response: IpcResponse) {
        let sender = self.lock().waiting.remove(&response.id);
        if let Some(sender) = sender {
            let _ = sender.send(response.outcome.into_result());
        }
    }

    /// Fails every waiting call and all future ones with `error`. The first failure wins,
    /// so a deliberate close is not reported as a crash once the process exits.
    pub(crate) fn fail_all(&self, error: DriverError) {
        let waiting = {
            let mut state = self.lock();
            let failure = state.failure.get_or_insert(error).clone();
            state
                .waiting
                .drain()
                .map(|(_, sender)| (sender, failure.clone()))
                .collect::<Vec<_>>()
        };
        for (sender, failure) in waiting {
            let _ = sender.send(Err(failure));
        }
    }

    pub(crate) fn failure(&self) -> Option<DriverError> {
        self.lock().failure.clone()
    }

    fn lock(&self) -> MutexGuard<'_, PendingState> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_driver_api::ErrorKind;
    use serde_json::json;

    #[tokio::test]
    async fn responses_are_routed_by_id_in_any_order() {
        let pending = PendingCalls::default();
        let first = pending.register(1).unwrap();
        let second = pending.register(2).unwrap();
        pending.deliver(IpcResponse::success(2, json!("b")));
        pending.deliver(IpcResponse::success(1, json!("a")));
        assert_eq!(first.await.unwrap().unwrap(), json!("a"));
        assert_eq!(second.await.unwrap().unwrap(), json!("b"));
    }

    #[tokio::test]
    async fn failure_reaches_waiting_and_future_calls() {
        let pending = PendingCalls::default();
        let waiting = pending.register(1).unwrap();
        pending.fail_all(DriverError::connection("gone"));
        pending.fail_all(DriverError::internal("later"));
        assert_eq!(
            waiting.await.unwrap().unwrap_err().kind,
            ErrorKind::Connection
        );
        assert_eq!(pending.register(2).unwrap_err().message, "gone");
    }
}
