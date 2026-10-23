/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connect::ADMIN_DATABASE;
use crate::errors::map_error;
use mongodb::Client;
use mongodb::bson::{Bson, doc};
use relix_driver_api::DriverError;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::Notify;

#[derive(Clone)]
pub struct RunningCommand {
    pub tag: Option<Bson>,
    pub abort: Arc<Notify>,
}

/// Remembers the console command that is currently running so `cancel` can abort the
/// local wait immediately and kill the server-side operation by its comment tag.
#[derive(Default)]
pub struct CommandTracker {
    running: Mutex<Option<RunningCommand>>,
}

impl CommandTracker {
    pub fn begin(&self, tag: Option<Bson>) -> RunningCommand {
        let command = RunningCommand {
            tag,
            abort: Arc::new(Notify::new()),
        };
        *self.lock() = Some(command.clone());
        command
    }

    pub fn finish(&self) {
        self.lock().take();
    }

    pub fn current(&self) -> Option<RunningCommand> {
        self.lock().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<RunningCommand>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub fn operation_tag() -> Bson {
    Bson::String(format!("relix-{}", uuid::Uuid::new_v4()))
}

/// Kills the caller's own server operations carrying `tag` as their comment. `$ownOps`
/// restricts `currentOp` to operations of the authenticated user, which MongoDB lets
/// every user see and kill without the cluster-wide `killop` privilege.
pub async fn kill_tagged_operations(client: &Client, tag: &Bson) -> Result<u64, DriverError> {
    let admin = client.database(ADMIN_DATABASE);
    let reply = admin
        .run_command(doc! {
            "currentOp": 1,
            "$ownOps": true,
            "command.comment": tag.clone(),
        })
        .await
        .map_err(map_error)?;
    let operations = reply.get_array("inprog").map(Vec::as_slice).unwrap_or(&[]);
    let mut killed = 0;
    for operation in operations {
        let Some(id) = operation.as_document().and_then(|op| op.get("opid")) else {
            continue;
        };
        admin
            .run_command(doc! { "killOp": 1, "op": id.clone() })
            .await
            .map_err(map_error)?;
        killed += 1;
    }
    Ok(killed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_exposes_only_the_running_command() {
        let tracker = CommandTracker::default();
        assert!(tracker.current().is_none());
        let running = tracker.begin(Some(operation_tag()));
        let current = tracker.current().unwrap();
        assert_eq!(current.tag, running.tag);
        tracker.finish();
        assert!(tracker.current().is_none());
    }

    #[test]
    fn tags_are_unique() {
        assert_ne!(operation_tag(), operation_tag());
    }
}
