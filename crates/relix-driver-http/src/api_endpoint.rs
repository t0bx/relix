/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::endpoint::{HttpAuth, HttpEndpoint, client_builder, pin_tunneled_host};
use crate::errors::map_transport_error;
use relix_core::{ConnectionConfig, TlsMode, TlsSettings};
use relix_driver_api::DriverError;
use reqwest::header::CONTENT_LENGTH;
use reqwest::{Method, RequestBuilder, StatusCode, Url};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawResponse {
    pub status: StatusCode,
    pub body: String,
}

impl HttpEndpoint {
    /// Binds to a fixed API base URL (a cloud provider's REST endpoint or an explicit
    /// override) without probing. These APIs receive bearer credentials, so HTTPS always
    /// verifies the certificate chain and host name, whatever the TLS mode says; plain
    /// HTTP is only accepted when TLS is explicitly disabled (local emulators, test
    /// servers). Behind an SSH tunnel the URL host is pinned to the forwarded address.
    pub async fn at_base_url(
        config: &ConnectionConfig,
        base: Url,
        auth: HttpAuth,
        request_timeout: Option<Duration>,
    ) -> Result<Self, DriverError> {
        relix_tls::install_default_provider();
        let mut builder = client_builder(config, request_timeout);
        match base.scheme() {
            "https" => {
                let verified = TlsSettings {
                    mode: TlsMode::VerifyFull,
                    ca_certificate: config.settings.tls.ca_certificate.clone(),
                };
                let tls = relix_tls::client_config(&verified)
                    .await?
                    .ok_or_else(|| DriverError::internal("TLS configuration is missing"))?;
                builder = builder.tls_backend_preconfigured(tls);
            }
            "http" if config.settings.tls.mode == TlsMode::Disable => {}
            "http" => {
                return Err(DriverError::configuration(
                    "plain HTTP endpoints are only used when TLS is set to \"disable\"",
                ));
            }
            other => {
                return Err(DriverError::configuration(format!(
                    "unsupported endpoint scheme \"{other}\""
                )));
            }
        }
        let host = base.host_str().unwrap_or_default().to_owned();
        let client = pin_tunneled_host(builder, config, &host)
            .build()
            .map_err(|error| DriverError::configuration(error.to_string()))?;
        Ok(Self::from_parts(client, with_trailing_slash(base), auth))
    }

    /// Builds a URL below the base from raw path segments, percent-encoding each one so
    /// identifiers with spaces, slashes or unicode characters stay a single segment.
    pub fn segments_url(&self, segments: &[&str]) -> Url {
        let mut url = self.base_url().clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty().extend(segments);
        }
        url
    }

    /// A `POST` without payload. HTTP clients omit the length of an empty body, which
    /// Google front ends reject with `411 Length Required`, so it is sent explicitly.
    pub fn empty_post(&self, url: Url) -> RequestBuilder {
        self.request_url(Method::POST, url)
            .header(CONTENT_LENGTH, "0")
    }

    /// Sends the request and returns status and body for every HTTP status, leaving the
    /// interpretation of error payloads to the caller. Only transport failures are errors.
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
}

fn with_trailing_slash(mut base: Url) -> Url {
    if !base.path().ends_with('/') {
        let path = format!("{}/", base.path());
        base.set_path(&path);
    }
    base
}
