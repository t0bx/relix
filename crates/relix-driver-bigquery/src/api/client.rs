/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::auth::TokenSource;
use crate::errors::{failure, unexpected_reply};
use relix_driver_api::DriverError;
use relix_driver_http::header::ACCEPT;
use relix_driver_http::{HttpEndpoint, Method};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub(crate) type QueryParams<'a> = [(&'a str, String)];

/// Authenticated JSON client for the BigQuery REST API v2. A fresh (cached) access token
/// is attached to every request so long sessions survive token rotation.
pub(crate) struct BigQueryClient {
    endpoint: HttpEndpoint,
    tokens: TokenSource,
}

impl BigQueryClient {
    pub(crate) fn new(endpoint: HttpEndpoint, tokens: TokenSource) -> Self {
        Self { endpoint, tokens }
    }

    pub(crate) async fn get<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        query: &QueryParams<'_>,
        statement: Option<&str>,
    ) -> Result<T, DriverError> {
        self.send(Method::GET, segments, query, None::<&()>, statement)
            .await
    }

    pub(crate) async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        segments: &[&str],
        query: &QueryParams<'_>,
        body: Option<&B>,
        statement: Option<&str>,
    ) -> Result<T, DriverError> {
        self.send(Method::POST, segments, query, body, statement)
            .await
    }

    async fn send<T: DeserializeOwned, B: Serialize>(
        &self,
        method: Method,
        segments: &[&str],
        query: &QueryParams<'_>,
        body: Option<&B>,
        statement: Option<&str>,
    ) -> Result<T, DriverError> {
        let mut url = self.endpoint.segments_url(segments);
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (name, value) in query {
                pairs.append_pair(name, value);
            }
        }
        let endpoint = self.endpoint.with_auth(self.tokens.authorization().await?);
        let request = match body {
            Some(body) => endpoint.request_url(method, url).json(body),
            None if method == Method::POST => endpoint.empty_post(url),
            None => endpoint.request_url(method, url),
        }
        .header(ACCEPT, "application/json");
        let response = endpoint.exchange(request).await?;
        if !response.status.is_success() {
            return Err(failure(response.status, &response.body, statement));
        }
        serde_json::from_str(&response.body).map_err(unexpected_reply)
    }
}
