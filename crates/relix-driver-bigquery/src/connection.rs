/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::catalog::first_dataset_page;
use crate::api::client::BigQueryClient;
use crate::api::jobs::{WAIT_MS, cancel_job, run_query};
use crate::api::models::{
    ConnectionProperty, DatasetReference, FormatOptions, JobReference, QueryRequest, QueryResults,
};
use crate::auth::TokenSource;
use crate::settings::BigQuerySettings;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect};
use relix_driver_api::{
    DatabaseConnection, DriverError, ParamStyle, ServerInfo, SqlBuilder, SqlConnection,
};
use relix_driver_http::{HttpAuth, HttpEndpoint, RunningJob};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

const PRODUCT_NAME: &str = "Google BigQuery";
const API_VERSION: &str = "REST API v2";
const SESSION_PROPERTY: &str = "session_id";
const ABORT_SESSION_SQL: &str = "CALL BQ.ABORT_SESSION()";

struct QueryDefaults {
    location: Option<String>,
    dataset: Option<DatasetReference>,
    maximum_bytes_billed: Option<u64>,
    job_timeout_ms: Option<u64>,
}

/// A BigQuery project reached through the REST API v2. Editor statements share a
/// BigQuery session (created with the first statement) so temporary tables, variables
/// and multi-statement transactions carry over; browsing runs outside the session.
pub struct BigQueryConnection {
    client: BigQueryClient,
    pub(crate) project: String,
    pub(crate) additional_projects: Vec<String>,
    defaults: QueryDefaults,
    pub(crate) running: RunningJob<JobReference>,
    session: Mutex<Option<String>>,
    pub(crate) builder: SqlBuilder,
    closed: AtomicBool,
}

impl BigQueryConnection {
    pub(crate) async fn open(config: &ConnectionConfig) -> Result<Self, DriverError> {
        let settings = BigQuerySettings::from_config(config)?;
        let tokens = TokenSource::load(&settings.auth).await?;
        let project = match settings.project.clone() {
            Some(project) => project,
            None => tokens.project_id().await.ok_or_else(|| {
                DriverError::configuration(
                    "set the \"project_id\" property; the credentials name no project",
                )
            })?,
        };
        let endpoint =
            HttpEndpoint::at_base_url(config, settings.api_base.clone(), HttpAuth::None, None)
                .await?;
        let connection = Self {
            client: BigQueryClient::new(endpoint, tokens),
            defaults: QueryDefaults {
                location: settings.location,
                dataset: settings.dataset.map(|dataset_id| DatasetReference {
                    project_id: project.clone(),
                    dataset_id,
                }),
                maximum_bytes_billed: settings.maximum_bytes_billed,
                job_timeout_ms: settings.job_timeout_ms,
            },
            project,
            additional_projects: settings.additional_projects,
            running: RunningJob::default(),
            session: Mutex::new(None),
            builder: SqlBuilder::new(Dialect::BigQuery, ParamStyle::Inline),
            closed: AtomicBool::new(false),
        };
        first_dataset_page(&connection.client, &connection.project).await?;
        Ok(connection)
    }

    pub(crate) fn client(&self) -> &BigQueryClient {
        &self.client
    }

    /// Runs a query outside the editor session (introspection, table pages).
    pub(crate) async fn query(
        &self,
        sql: &str,
        max_results: u32,
    ) -> Result<QueryResults, DriverError> {
        self.ensure_open()?;
        let request = self.request(sql, max_results);
        run_query(&self.client, &self.project, &request, None).await
    }

    /// Runs an editor statement inside the connection's BigQuery session, creating the
    /// session with the first statement. Servers that do not return session information
    /// (e.g. emulators) are used without a session.
    pub(crate) async fn query_in_session(
        &self,
        sql: &str,
        max_results: u32,
    ) -> Result<QueryResults, DriverError> {
        self.ensure_open()?;
        let session = self.session_id();
        let mut request = self.request(sql, max_results);
        match &session {
            Some(id) => request.connection_properties = session_property(id),
            None => request.create_session = true,
        }
        let results = run_query(&self.client, &self.project, &request, Some(&self.running)).await?;
        if session.is_none()
            && let Some(id) = results
                .session_info
                .as_ref()
                .and_then(|info| info.session_id.clone())
        {
            *self.lock_session() = Some(id);
        }
        Ok(results)
    }

    fn request(&self, sql: &str, max_results: u32) -> QueryRequest {
        QueryRequest {
            query: sql.to_owned(),
            use_legacy_sql: false,
            max_results,
            timeout_ms: WAIT_MS,
            request_id: Uuid::new_v4().to_string(),
            format_options: FormatOptions {
                use_int64_timestamp: true,
            },
            location: self.defaults.location.clone(),
            default_dataset: self.defaults.dataset.clone(),
            create_session: false,
            connection_properties: Vec::new(),
            maximum_bytes_billed: self
                .defaults
                .maximum_bytes_billed
                .map(|bytes| bytes.to_string()),
            job_timeout_ms: self.defaults.job_timeout_ms.map(|ms| ms.to_string()),
        }
    }

    fn ensure_open(&self) -> Result<(), DriverError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(DriverError::connection("the connection is closed"));
        }
        Ok(())
    }

    fn session_id(&self) -> Option<String> {
        self.lock_session().clone()
    }

    fn lock_session(&self) -> MutexGuard<'_, Option<String>> {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    async fn abort_session(&self, id: &str) {
        let mut request = self.request(ABORT_SESSION_SQL, 1);
        request.connection_properties = session_property(id);
        let _ = run_query(&self.client, &self.project, &request, None).await;
    }
}

fn session_property(id: &str) -> Vec<ConnectionProperty> {
    vec![ConnectionProperty {
        key: SESSION_PROPERTY.to_owned(),
        value: id.to_owned(),
    }]
}

#[async_trait]
impl DatabaseConnection for BigQueryConnection {
    fn server_info(&self) -> ServerInfo {
        ServerInfo {
            product: PRODUCT_NAME.to_owned(),
            version: API_VERSION.to_owned(),
        }
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.ensure_open()?;
        first_dataset_page(&self.client, &self.project).await
    }

    async fn close(&self) -> Result<(), DriverError> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        if let Some(reference) = self.running.request_cancel() {
            let _ = cancel_job(&self.client, &reference).await;
        }
        let session = self.lock_session().take();
        if let Some(id) = session {
            self.abort_session(&id).await;
        }
        Ok(())
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}
