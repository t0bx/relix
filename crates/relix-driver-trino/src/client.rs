/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::protocol::{QueryResults, ServerDescription};
use crate::session_state::SessionState;
use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::{
    HttpEndpoint, Method, RequestBuilder, Response, Url, map_status_error, map_transport_error,
};
use std::time::Duration;

const STATEMENT_PATH: &str = "v1/statement";
const INFO_PATH: &str = "v1/info";
const SOURCE: &str = "relix";
const CLIENT_CAPABILITIES: &str = "PARAMETRIC_DATETIME";
const BUSY_RETRIES: u32 = 50;
const BUSY_PAUSE: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub(crate) struct TrinoClient {
    endpoint: HttpEndpoint,
    user: String,
}

impl TrinoClient {
    pub(crate) fn new(endpoint: HttpEndpoint, user: String) -> Self {
        Self { endpoint, user }
    }

    pub(crate) async fn server_description(&self) -> Result<ServerDescription, DriverError> {
        let request = self.request(Method::GET, INFO_PATH);
        self.endpoint.json(request).await
    }

    pub(crate) async fn submit(
        &self,
        sql: &str,
        session: &SessionState,
    ) -> Result<Response, DriverError> {
        let request = session
            .apply(self.request(Method::POST, STATEMENT_PATH))
            .body(sql.to_owned());
        self.send(request, false).await
    }

    pub(crate) async fn next_page(&self, next_uri: &str) -> Result<Response, DriverError> {
        let target = self.local_uri(next_uri);
        self.send(self.request(Method::GET, &target), true).await
    }

    /// Cancels the query (or abandons the rest of its result) by deleting its next URI.
    pub(crate) async fn abandon(&self, next_uri: &str) -> Result<(), DriverError> {
        let target = self.local_uri(next_uri);
        self.send(self.request(Method::DELETE, &target), false)
            .await
            .map(drop)
    }

    pub(crate) async fn page(&self, response: Response) -> Result<QueryResults, DriverError> {
        let body = response
            .text()
            .await
            .map_err(|error| map_transport_error(&error))?;
        serde_json::from_str(&body).map_err(|error| {
            DriverError::new(ErrorKind::Protocol, "unexpected response from Trino")
                .with_detail(error.to_string())
        })
    }

    fn request(&self, method: Method, target: &str) -> RequestBuilder {
        self.endpoint
            .request(method, target)
            .header("X-Trino-User", self.user.as_str())
            .header("X-Trino-Source", SOURCE)
            .header("X-Trino-Client-Capabilities", CLIENT_CAPABILITIES)
    }

    /// Sends a request; result pages answered with `503 Service Unavailable` (the
    /// coordinator is busy) are retried after a short pause, as the protocol asks.
    async fn send(
        &self,
        request: RequestBuilder,
        retry_busy: bool,
    ) -> Result<Response, DriverError> {
        let mut attempts = 0;
        loop {
            let attempt = request
                .try_clone()
                .ok_or_else(|| DriverError::internal("request cannot be repeated"))?;
            let response = attempt
                .send()
                .await
                .map_err(|error| map_transport_error(&error))?;
            let status = response.status().as_u16();
            if response.status().is_success() {
                return Ok(response);
            }
            if retry_busy && status == 503 && attempts < BUSY_RETRIES {
                attempts += 1;
                tokio::time::sleep(BUSY_PAUSE).await;
                continue;
            }
            let body = response.text().await.unwrap_or_default();
            return Err(map_status_error(status, &body));
        }
    }

    /// Points a `nextUri` at the address this client connected to: behind a proxy or an
    /// SSH tunnel the coordinator advertises an address the client cannot reach.
    fn local_uri(&self, next_uri: &str) -> String {
        let base = self.endpoint.base_url();
        let Ok(mut target) = Url::parse(next_uri) else {
            return next_uri.to_owned();
        };
        let rebased = target.set_scheme(base.scheme()).is_ok()
            && target.set_host(base.host_str()).is_ok()
            && target.set_port(base.port_or_known_default()).is_ok();
        if rebased {
            target.to_string()
        } else {
            next_uri.to_owned()
        }
    }
}
