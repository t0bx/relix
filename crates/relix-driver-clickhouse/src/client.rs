/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::credentials::Credentials;
use crate::errors::{READONLY, SESSION_IS_LOCKED, ServerException};
use crate::result_reader::{QueryOutcome, read_result};
use relix_core::{Dialect, RowBatch};
use relix_driver_api::DriverError;
use relix_driver_http::{HttpEndpoint, Method, Response, map_transport_error};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use uuid::Uuid;

const RESULT_FORMAT: &str = "JSONCompactEachRowWithNamesAndTypes";
const FORMAT_SETTINGS: [(&str, &str); 1] = [("output_format_json_quote_denormals", "1")];
const EXCEPTION_CODE_HEADER: &str = "X-ClickHouse-Exception-Code";
const SESSION_LOCK_RETRIES: u32 = 30;
const SESSION_LOCK_PAUSE: Duration = Duration::from_millis(100);

pub(crate) struct SessionParams<'a> {
    pub(crate) id: &'a str,
    pub(crate) timeout_secs: u64,
}

pub(crate) struct QueryRequest<'a> {
    pub(crate) sql: &'a str,
    pub(crate) query_id: String,
    pub(crate) session: Option<SessionParams<'a>>,
    pub(crate) params: Vec<(&'a str, String)>,
}

impl<'a> QueryRequest<'a> {
    pub(crate) fn new(sql: &'a str) -> Self {
        Self {
            sql,
            query_id: Uuid::new_v4().to_string(),
            session: None,
            params: Vec::new(),
        }
    }

    pub(crate) fn param(mut self, name: &'a str, value: impl Into<String>) -> Self {
        self.params.push((name, value.into()));
        self
    }
}

#[derive(Clone)]
pub(crate) struct ClickHouseClient {
    endpoint: HttpEndpoint,
    credentials: Credentials,
    database: Option<String>,
    format_settings: Arc<AtomicBool>,
}

impl ClickHouseClient {
    pub(crate) fn new(
        endpoint: HttpEndpoint,
        credentials: Credentials,
        database: Option<String>,
    ) -> Self {
        Self {
            endpoint,
            credentials,
            database,
            format_settings: Arc::new(AtomicBool::new(true)),
        }
    }

    pub(crate) fn database(&self) -> Option<&str> {
        self.database.as_deref()
    }

    pub(crate) async fn query(
        &self,
        request: &QueryRequest<'_>,
        limit: usize,
    ) -> Result<QueryOutcome, DriverError> {
        let response = self.send(request).await?;
        read_result(response, limit, request.sql).await
    }

    pub(crate) async fn rows(&self, request: &QueryRequest<'_>) -> Result<RowBatch, DriverError> {
        let outcome = self.query(request, usize::MAX).await?;
        Ok(outcome.batch.unwrap_or_default())
    }

    pub(crate) async fn kill(&self, query_id: &str) -> Result<(), DriverError> {
        let sql = format!(
            "KILL QUERY WHERE query_id = {} ASYNC",
            Dialect::ClickHouse.quote_literal(query_id)
        );
        self.rows(&QueryRequest::new(&sql)).await.map(drop)
    }

    /// Sends the request and turns error responses into driver errors. Two failures are
    /// retried: a read-only user rejecting the output format settings (they are then
    /// dropped for this connection), and a session still locked by a query that is
    /// shutting down after it was cancelled or truncated.
    async fn send(&self, request: &QueryRequest<'_>) -> Result<Response, DriverError> {
        let mut lock_retries = 0;
        loop {
            let response = self
                .build(request)
                .send()
                .await
                .map_err(|error| map_transport_error(&error))?;
            let status = response.status();
            if status.is_success() {
                return Ok(response);
            }
            let code_header = response
                .headers()
                .get(EXCEPTION_CODE_HEADER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse().ok());
            let body = response.text().await.unwrap_or_default();
            let exception = ServerException::from_body(code_header, &body);
            match exception.code {
                Some(READONLY) if self.format_settings.swap(false, Ordering::SeqCst) => {}
                Some(SESSION_IS_LOCKED) if lock_retries < SESSION_LOCK_RETRIES => {
                    lock_retries += 1;
                    tokio::time::sleep(SESSION_LOCK_PAUSE).await;
                }
                _ => return Err(exception.into_error(Some(status.as_u16()), Some(request.sql))),
            }
        }
    }

    fn build(&self, request: &QueryRequest<'_>) -> relix_driver_http::RequestBuilder {
        let mut query: Vec<(String, String)> = vec![
            ("query_id".into(), request.query_id.clone()),
            ("default_format".into(), RESULT_FORMAT.into()),
        ];
        match &request.session {
            Some(session) => {
                query.push(("session_id".into(), session.id.to_owned()));
                query.push(("session_timeout".into(), session.timeout_secs.to_string()));
            }
            None => {
                if let Some(database) = &self.database {
                    query.push(("database".into(), database.clone()));
                }
            }
        }
        if self.format_settings.load(Ordering::SeqCst) {
            for (name, value) in FORMAT_SETTINGS {
                query.push((name.into(), value.into()));
            }
        }
        for (name, value) in &request.params {
            query.push((format!("param_{name}"), value.clone()));
        }
        let mut url = self.endpoint.url("");
        url.query_pairs_mut().extend_pairs(query);
        let target = format!("?{}", url.query().unwrap_or_default());
        let builder = self
            .endpoint
            .request(Method::POST, &target)
            .body(request.sql.to_owned());
        self.credentials.apply(builder)
    }
}
