/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::settings::AuthMode;
use gcp_auth::{CustomServiceAccount, TokenProvider};
use relix_core::Secret;
use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::HttpAuth;
use std::sync::Arc;

const BIGQUERY_SCOPE: &str = "https://www.googleapis.com/auth/bigquery";

/// Source of OAuth access tokens for BigQuery requests. Google credentials are resolved
/// by `gcp_auth`, which caches tokens in memory and refreshes them before they expire;
/// nothing is written to disk.
pub(crate) enum TokenSource {
    Google(Arc<dyn TokenProvider>),
    Anonymous,
}

impl TokenSource {
    pub(crate) async fn load(mode: &AuthMode) -> Result<Self, DriverError> {
        match mode {
            AuthMode::Anonymous => Ok(Self::Anonymous),
            AuthMode::ServiceAccountFile(path) => {
                let path = path.clone();
                let account =
                    tokio::task::spawn_blocking(move || CustomServiceAccount::from_file(path))
                        .await
                        .map_err(|error| DriverError::internal(error.to_string()))?
                        .map_err(|error| {
                            credentials_error("cannot load the service account key file", &error)
                        })?;
                Ok(Self::Google(Arc::new(account)))
            }
            AuthMode::ApplicationDefault => {
                gcp_auth::provider()
                    .await
                    .map(Self::Google)
                    .map_err(|error| {
                        credentials_error("no Google Application Default Credentials found", &error)
                            .with_detail(
                                "run `gcloud auth application-default login`, set \
                             GOOGLE_APPLICATION_CREDENTIALS to a service account key file or \
                             set the \"credentials_file\" property",
                            )
                    })
            }
        }
    }

    pub(crate) async fn authorization(&self) -> Result<HttpAuth, DriverError> {
        match self {
            Self::Anonymous => Ok(HttpAuth::None),
            Self::Google(provider) => {
                let token = provider.token(&[BIGQUERY_SCOPE]).await.map_err(|error| {
                    credentials_error("cannot obtain a Google access token", &error)
                })?;
                Ok(HttpAuth::Bearer(Secret::new(token.as_str())))
            }
        }
    }

    pub(crate) async fn project_id(&self) -> Option<String> {
        match self {
            Self::Anonymous => None,
            Self::Google(provider) => provider
                .project_id()
                .await
                .ok()
                .map(|project| project.to_string()),
        }
    }
}

fn credentials_error(message: &str, error: &gcp_auth::Error) -> DriverError {
    DriverError::new(ErrorKind::Authentication, format!("{message}: {error}"))
}
