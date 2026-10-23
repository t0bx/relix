/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::models::{QueryStatus, ResultSet, StatementRequest};
use crate::auth::Credentials;
use crate::errors::{failure, unexpected_reply};
use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::header::{ACCEPT, CONTENT_TYPE};
use relix_driver_http::{
    HttpEndpoint, Method, PollBackoff, RawResponse, RequestBuilder, RunningJob, StatusCode, Url,
};
use uuid::Uuid;

enum Reply {
    Finished(ResultSet),
    Running(String),
}

/// Client for the Snowflake SQL API v2 (`/api/v2/statements`).
pub(crate) struct SqlApiClient {
    endpoint: HttpEndpoint,
    credentials: Credentials,
}

impl SqlApiClient {
    pub(crate) fn new(endpoint: HttpEndpoint, credentials: Credentials) -> Self {
        Self {
            endpoint,
            credentials,
        }
    }

    /// Submits one statement and waits until it finished, returning its first result
    /// partition. With a job tracker the statement is submitted asynchronously, so its
    /// handle is known immediately and a concurrent cancel can reach it; without one the
    /// API answers synchronously when the statement completes within its wait window.
    /// Running statements are polled with a growing delay.
    pub(crate) async fn execute(
        &self,
        request: &StatementRequest<'_>,
        job: Option<&RunningJob<String>>,
    ) -> Result<ResultSet, DriverError> {
        let mut url = self.statements_url(&[]);
        url.query_pairs_mut()
            .append_pair("requestId", &Uuid::new_v4().to_string());
        if job.is_some() {
            url.query_pairs_mut().append_pair("async", "true");
        }
        let body = serde_json::to_vec(request)
            .map_err(|error| DriverError::internal(format!("cannot encode request: {error}")))?;
        let submit = self
            .endpoint
            .request_url(Method::POST, url)
            .header(CONTENT_TYPE, "application/json")
            .body(body);
        let mut reply = self.send(submit, Some(request.statement)).await?;
        let mut backoff = PollBackoff::default();
        let mut attached = false;
        loop {
            let handle = match reply {
                Reply::Finished(result) => return Ok(result),
                Reply::Running(handle) => handle,
            };
            if let Some(job) = job.filter(|_| !attached) {
                attached = true;
                if job.attach(handle.clone()) {
                    let _ = self.cancel(&handle).await;
                }
            }
            backoff.wait().await;
            let poll = self
                .endpoint
                .request_url(Method::GET, self.statement_url(&handle));
            reply = self.send(poll, Some(request.statement)).await?;
        }
    }

    pub(crate) async fn partition(
        &self,
        handle: &str,
        index: usize,
    ) -> Result<ResultSet, DriverError> {
        let mut url = self.statement_url(handle);
        url.query_pairs_mut()
            .append_pair("partition", &index.to_string());
        match self
            .send(self.endpoint.request_url(Method::GET, url), None)
            .await?
        {
            Reply::Finished(result) => Ok(result),
            Reply::Running(_) => Err(DriverError::new(
                ErrorKind::Protocol,
                "Snowflake reported a finished statement as running",
            )),
        }
    }

    pub(crate) async fn cancel(&self, handle: &str) -> Result<(), DriverError> {
        let url = self.statements_url(&[handle, "cancel"]);
        let response = self.exchange(self.endpoint.empty_post(url)).await?;
        if response.status.is_success() {
            return Ok(());
        }
        Err(failure(response.status, &response.body, None))
    }

    fn statement_url(&self, handle: &str) -> Url {
        self.statements_url(&[handle])
    }

    fn statements_url(&self, tail: &[&str]) -> Url {
        let mut segments = vec!["api", "v2", "statements"];
        segments.extend_from_slice(tail);
        self.endpoint.segments_url(&segments)
    }

    async fn send(
        &self,
        request: RequestBuilder,
        statement: Option<&str>,
    ) -> Result<Reply, DriverError> {
        let response = self.exchange(request).await?;
        match response.status {
            StatusCode::OK => serde_json::from_str(&response.body)
                .map(Reply::Finished)
                .map_err(unexpected_reply),
            StatusCode::ACCEPTED => {
                let status: QueryStatus =
                    serde_json::from_str(&response.body).map_err(unexpected_reply)?;
                status.statement_handle.map(Reply::Running).ok_or_else(|| {
                    DriverError::new(
                        ErrorKind::Protocol,
                        "Snowflake accepted the statement without a statement handle",
                    )
                })
            }
            status => Err(failure(status, &response.body, statement)),
        }
    }

    async fn exchange(&self, request: RequestBuilder) -> Result<RawResponse, DriverError> {
        let request = self
            .credentials
            .authorize(request.header(ACCEPT, "application/json"))?;
        self.endpoint.exchange(request).await
    }
}
