/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::{ClickHouseClient, QueryRequest, SessionParams};
use crate::result_reader::QueryOutcome;
use relix_core::{ConnectionSettings, Dialect};
use relix_driver_api::DriverError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use uuid::Uuid;

const SESSION_TIMEOUT_PROPERTY: &str = "session_timeout";
const DEFAULT_SESSION_TIMEOUT_SECS: u64 = 3_600;

/// The editor's ClickHouse session: `SET`, `USE` and temporary tables persist between
/// statements because every editor request carries the same `session_id`. The `database`
/// URL parameter only applies to a single request, so the data source's database is
/// selected once with `USE` inside the session, where a later `USE` can change it.
pub(crate) struct EditorSession {
    id: String,
    timeout_secs: u64,
    started: AtomicBool,
    running: Mutex<Option<String>>,
}

impl EditorSession {
    pub(crate) fn new(settings: &ConnectionSettings) -> Result<Self, DriverError> {
        let timeout_secs = match settings.property(SESSION_TIMEOUT_PROPERTY) {
            None => DEFAULT_SESSION_TIMEOUT_SECS,
            Some(value) => value.parse().map_err(|_| {
                DriverError::configuration(format!(
                    "session_timeout must be a number of seconds, got \"{value}\""
                ))
            })?,
        };
        Ok(Self {
            id: format!("relix-{}", Uuid::new_v4()),
            timeout_secs,
            started: AtomicBool::new(false),
            running: Mutex::new(None),
        })
    }

    /// Runs one editor statement. A truncated result is killed in the background so
    /// the server stops producing rows that are never read.
    pub(crate) async fn run(
        &self,
        client: &ClickHouseClient,
        sql: &str,
        limit: usize,
    ) -> Result<QueryOutcome, DriverError> {
        if !self.started.load(Ordering::SeqCst) {
            if let Some(database) = client.database() {
                let use_database =
                    format!("USE {}", Dialect::ClickHouse.quote_identifier(database));
                client.query(&self.request(&use_database), 0).await?;
            }
            self.started.store(true, Ordering::SeqCst);
        }
        let request = self.request(sql);
        self.set_running(Some(request.query_id.clone()));
        let outcome = client.query(&request, limit).await;
        self.set_running(None);
        let outcome = outcome?;
        if outcome.truncated {
            let client = client.clone();
            let query_id = request.query_id;
            tokio::spawn(async move {
                let _ = client.kill(&query_id).await;
            });
        }
        Ok(outcome)
    }

    pub(crate) async fn cancel(&self, client: &ClickHouseClient) -> Result<(), DriverError> {
        let running = self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        match running {
            Some(query_id) => client.kill(&query_id).await,
            None => Ok(()),
        }
    }

    fn request<'a>(&'a self, sql: &'a str) -> QueryRequest<'a> {
        let mut request = QueryRequest::new(sql);
        request.session = Some(SessionParams {
            id: &self.id,
            timeout_secs: self.timeout_secs,
        });
        request
    }

    fn set_running(&self, query_id: Option<String>) {
        *self.running.lock().unwrap_or_else(PoisonError::into_inner) = query_id;
    }
}
