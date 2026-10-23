/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::SearchClient;
use crate::paths::segment;
use relix_driver_api::DriverError;
use relix_driver_http::Method;
use serde_json::Value as Json;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::Notify;

pub const OPAQUE_ID_HEADER: &str = "X-Opaque-Id";

#[derive(Debug, Clone)]
pub struct RunningRequest {
    pub opaque_id: String,
    pub abort: Arc<Notify>,
}

/// Tracks the console request in flight. Every request is tagged with a unique
/// `X-Opaque-Id`, which the tasks API reports, so a cancel can find and cancel exactly
/// the server tasks this request started.
#[derive(Default)]
pub struct RunningRequests {
    current: Mutex<Option<RunningRequest>>,
}

impl RunningRequests {
    pub fn begin(&self) -> RunningRequest {
        let running = RunningRequest {
            opaque_id: format!("relix-{}", uuid::Uuid::new_v4()),
            abort: Arc::new(Notify::new()),
        };
        *self.slot() = Some(running.clone());
        running
    }

    pub fn finish(&self, running: &RunningRequest) {
        let mut slot = self.slot();
        if slot
            .as_ref()
            .is_some_and(|current| current.opaque_id == running.opaque_id)
        {
            *slot = None;
        }
    }

    pub fn take(&self) -> Option<RunningRequest> {
        self.slot().take()
    }

    fn slot(&self) -> std::sync::MutexGuard<'_, Option<RunningRequest>> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Cancels the cancellable server tasks (searches, `_by_query`, reindex, ...) that carry
/// the request's opaque id and returns how many were cancelled.
pub async fn cancel_server_tasks(
    client: &SearchClient,
    opaque_id: &str,
) -> Result<usize, DriverError> {
    let tasks = client
        .json(Method::GET, "/_tasks?detailed=true", None)
        .await?;
    let mut cancelled = 0;
    for task_id in matching_tasks(&tasks, opaque_id) {
        let path = format!("/_tasks/{}/_cancel", segment(&task_id)?);
        if client.json(Method::POST, &path, None).await.is_ok() {
            cancelled += 1;
        }
    }
    Ok(cancelled)
}

fn matching_tasks(tasks: &Json, opaque_id: &str) -> Vec<String> {
    let nodes = tasks["nodes"]
        .as_object()
        .into_iter()
        .flat_map(|nodes| nodes.values());
    nodes
        .filter_map(|node| node["tasks"].as_object())
        .flat_map(|tasks| tasks.iter())
        .filter(|(_, task)| {
            task["cancellable"].as_bool() == Some(true)
                && task["parent_task_id"].is_null()
                && task["headers"][OPAQUE_ID_HEADER].as_str() == Some(opaque_id)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_cancellable_parent_tasks_with_our_id_match() {
        let tasks = json!({"nodes": {"n1": {"tasks": {
            "n1:10": {"cancellable": true, "headers": {"X-Opaque-Id": "relix-a"}},
            "n1:11": {"cancellable": true, "parent_task_id": "n1:10",
                      "headers": {"X-Opaque-Id": "relix-a"}},
            "n1:12": {"cancellable": false, "headers": {"X-Opaque-Id": "relix-a"}},
            "n1:13": {"cancellable": true, "headers": {"X-Opaque-Id": "other"}}
        }}}});
        assert_eq!(matching_tasks(&tasks, "relix-a"), vec!["n1:10".to_owned()]);
        assert!(matching_tasks(&json!({}), "relix-a").is_empty());
    }

    #[test]
    fn finishing_clears_only_the_own_request() {
        let running = RunningRequests::default();
        let first = running.begin();
        let second = running.begin();
        running.finish(&first);
        assert_eq!(
            running.take().map(|request| request.opaque_id),
            Some(second.opaque_id)
        );
        assert!(running.take().is_none());
    }
}
