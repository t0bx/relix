/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio::task::JoinHandle;
use tokio_postgres::Client;

const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

pub(crate) struct ManagedClient {
    client: RwLock<Client>,
    connection_task: JoinHandle<()>,
}

impl ManagedClient {
    pub(crate) fn new(client: Client, connection_task: JoinHandle<()>) -> Self {
        Self {
            client: RwLock::new(client),
            connection_task,
        }
    }

    pub(crate) async fn read(&self) -> RwLockReadGuard<'_, Client> {
        self.client.read().await
    }

    pub(crate) async fn write(&self) -> RwLockWriteGuard<'_, Client> {
        self.client.write().await
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.connection_task.is_finished()
    }

    /// Releases a shared client: when nobody else holds it, the client is dropped so the
    /// connection task can send `Terminate` and finish; otherwise (or when it lingers) the
    /// connection task is aborted, which also fails every in-flight query.
    pub(crate) async fn release(shared: Arc<Self>) {
        match Arc::try_unwrap(shared) {
            Ok(managed) => {
                let Self {
                    client,
                    mut connection_task,
                } = managed;
                drop(client);
                if tokio::time::timeout(SHUTDOWN_GRACE, &mut connection_task)
                    .await
                    .is_err()
                {
                    connection_task.abort();
                }
            }
            Err(shared) => shared.connection_task.abort(),
        }
    }
}
