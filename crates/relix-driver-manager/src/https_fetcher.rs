/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ManagerError;
use crate::fetcher::{DownloadProgress, PackageFetcher, ensure_https};
use async_trait::async_trait;
use rustls::ClientConfig;
use rustls::crypto::CryptoProvider;
use std::sync::Arc;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(60);
const INDEX_TIMEOUT: Duration = Duration::from_secs(60);

pub struct HttpsFetcher {
    client: reqwest::Client,
}

impl HttpsFetcher {
    pub fn new() -> Result<Self, ManagerError> {
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls_config()?)
            .https_only(true)
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .user_agent(concat!("relix/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| ManagerError::Download(error.to_string()))?;
        Ok(Self { client })
    }

    async fn get(
        &self,
        url: &str,
        timeout: Option<Duration>,
        max_bytes: u64,
        progress: DownloadProgress<'_>,
    ) -> Result<Vec<u8>, ManagerError> {
        ensure_https(url)?;
        let mut request = self.client.get(url);
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        let mut response = request
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(download_error)?;
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes)
        {
            return Err(ManagerError::TooLarge { limit: max_bytes });
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(download_error)? {
            if (body.len() + chunk.len()) as u64 > max_bytes {
                return Err(ManagerError::TooLarge { limit: max_bytes });
            }
            body.extend_from_slice(&chunk);
            progress(body.len() as u64);
        }
        Ok(body)
    }
}

#[async_trait]
impl PackageFetcher for HttpsFetcher {
    async fn fetch_index(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, ManagerError> {
        self.get(url, Some(INDEX_TIMEOUT), max_bytes, &|_| {}).await
    }

    async fn download(
        &self,
        url: &str,
        max_bytes: u64,
        progress: DownloadProgress<'_>,
    ) -> Result<Vec<u8>, ManagerError> {
        self.get(url, None, max_bytes, progress).await
    }
}

fn tls_config() -> Result<ClientConfig, ManagerError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = rustls_platform_verifier::Verifier::new(Arc::clone(&provider))
        .map_err(|error| ManagerError::Download(format!("TLS setup failed: {error}")))?;
    let config = ClientConfig::builder_with_provider(provider as Arc<CryptoProvider>)
        .with_safe_default_protocol_versions()
        .map_err(|error| ManagerError::Download(format!("TLS setup failed: {error}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok(config)
}

fn download_error(error: reqwest::Error) -> ManagerError {
    ManagerError::Download(error.without_url().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plain_http_is_rejected_before_any_request() {
        let fetcher = HttpsFetcher::new().unwrap();
        let result = fetcher
            .fetch_index("http://127.0.0.1:9/index.json", 1024)
            .await;
        assert!(matches!(result, Err(ManagerError::InsecureUrl(_))));
    }
}
