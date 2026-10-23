/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::{
    HttpEndpoint, Method, RequestBuilder, StatusCode, header, map_status_error, map_transport_error,
};
use serde_json::Value as Json;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawResponse {
    pub status: StatusCode,
    pub body: String,
}

impl RawResponse {
    pub fn into_error(self) -> DriverError {
        map_status_error(self.status.as_u16(), &self.body)
    }

    pub fn into_json(self) -> Result<Json, DriverError> {
        if !self.status.is_success() {
            return Err(self.into_error());
        }
        parse_json(&self.body)
    }
}

pub fn parse_json(body: &str) -> Result<Json, DriverError> {
    serde_json::from_str(body).map_err(|error| {
        DriverError::new(ErrorKind::Protocol, "the server sent invalid JSON")
            .with_detail(error.to_string())
    })
}

pub struct SearchClient {
    endpoint: HttpEndpoint,
    closed: AtomicBool,
}

impl SearchClient {
    pub fn new(endpoint: HttpEndpoint) -> Self {
        Self {
            endpoint,
            closed: AtomicBool::new(false),
        }
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    pub fn request(&self, method: Method, path: &str) -> Result<RequestBuilder, DriverError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(DriverError::connection("the connection was closed"));
        }
        Ok(self.endpoint.request(method, path))
    }

    pub async fn exchange(&self, request: RequestBuilder) -> Result<RawResponse, DriverError> {
        let response = request
            .send()
            .await
            .map_err(|error| map_transport_error(&error))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| map_transport_error(&error))?;
        Ok(RawResponse { status, body })
    }

    pub async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<&Json>,
    ) -> Result<RawResponse, DriverError> {
        let mut request = self.request(method, path)?;
        if let Some(body) = body {
            request = request
                .header(header::CONTENT_TYPE, "application/json")
                .body(body.to_string());
        }
        self.exchange(request).await
    }

    pub async fn json(
        &self,
        method: Method,
        path: &str,
        body: Option<&Json>,
    ) -> Result<Json, DriverError> {
        self.call(method, path, body).await?.into_json()
    }
}
