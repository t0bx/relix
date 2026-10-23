/*
 * @author t0bx
 * @since 23.10.2026
 */

use aws_config::timeout::TimeoutConfig;
use aws_config::{BehaviorVersion, ConfigLoader, Region, SdkConfig};
use aws_sdk_dynamodb::config::Credentials;
use aws_smithy_http_client::Builder;
use aws_smithy_http_client::tls::rustls_provider::CryptoMode;
use aws_smithy_http_client::tls::{Provider, TlsContext, TrustStore};
use relix_core::{ConnectionConfig, TlsMode};
use relix_driver_api::{DriverError, ErrorKind};
use std::path::Path;

pub const REGION_PROPERTY: &str = "region";
pub const PROFILE_PROPERTY: &str = "profile";
pub const ENDPOINT_PROPERTY: &str = "endpoint_url";
const CREDENTIALS_SOURCE: &str = "relix-connection";

/// Loads the AWS configuration for a data source. Credentials come from the standard
/// chain (environment, shared config/credentials files with the `profile` property,
/// SSO, credential processes, container and instance metadata) unless the username and
/// password fields hold an access key id and secret access key. `region` and
/// `endpoint_url` (DynamoDB Local, VPC endpoints) override the chain. Nothing is written
/// back to disk. HTTPS uses rustls with ring and the platform trust store plus the
/// optional CA file.
pub async fn load_sdk_config(config: &ConnectionConfig) -> Result<SdkConfig, DriverError> {
    let settings = &config.settings;
    let endpoint = settings.property(ENDPOINT_PROPERTY);
    if let Some(endpoint) = endpoint {
        check_endpoint(endpoint, settings.tls.mode)?;
    }
    let mut loader = aws_config::defaults(BehaviorVersion::latest())
        .http_client(
            Builder::new()
                .tls_provider(Provider::Rustls(CryptoMode::Ring))
                .tls_context(tls_context(settings.tls.ca_certificate.as_deref()).await?)
                .build_https(),
        )
        .timeout_config(timeouts(config));
    if let Some(profile) = settings.property(PROFILE_PROPERTY) {
        loader = loader.profile_name(profile);
    }
    if let Some(region) = settings.property(REGION_PROPERTY) {
        loader = loader.region(Region::new(region.to_owned()));
    }
    if let Some(endpoint) = endpoint {
        loader = loader.endpoint_url(endpoint);
    }
    loader = with_static_credentials(loader, config)?;
    let sdk_config = loader.load().await;
    if sdk_config.region().is_none() {
        return Err(DriverError::configuration(
            "no AWS region configured: set the 'region' property, AWS_REGION or a profile region",
        ));
    }
    Ok(sdk_config)
}

fn with_static_credentials(
    loader: ConfigLoader,
    config: &ConnectionConfig,
) -> Result<ConfigLoader, DriverError> {
    let access_key = config
        .settings
        .username
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty());
    let Some(access_key) = access_key else {
        return Ok(loader);
    };
    let Some(secret) = config
        .password
        .as_ref()
        .filter(|secret| !secret.expose().is_empty())
    else {
        return Err(DriverError::new(
            ErrorKind::Authentication,
            "an access key id needs its secret access key in the password field",
        ));
    };
    let credentials = Credentials::new(access_key, secret.expose(), None, None, CREDENTIALS_SOURCE);
    Ok(loader.credentials_provider(credentials))
}

fn timeouts(config: &ConnectionConfig) -> TimeoutConfig {
    let builder = TimeoutConfig::builder().connect_timeout(config.settings.connect_timeout());
    match config.settings.query_timeout() {
        Some(timeout) => builder.operation_timeout(timeout).build(),
        None => builder.build(),
    }
}

async fn tls_context(ca_certificate: Option<&Path>) -> Result<TlsContext, DriverError> {
    let mut trust_store = TrustStore::default();
    if let Some(path) = ca_certificate {
        let pem = tokio::fs::read(path).await.map_err(|error| {
            DriverError::new(
                ErrorKind::Tls,
                format!("cannot read CA certificate {}: {error}", path.display()),
            )
        })?;
        trust_store = trust_store.with_pem_certificate(pem);
    }
    TlsContext::builder()
        .with_trust_store(trust_store)
        .build()
        .map_err(|error| DriverError::new(ErrorKind::Tls, error.to_string()))
}

/// AWS endpoints always use verified HTTPS. A plain `http://` endpoint (DynamoDB Local)
/// is accepted unless the data source demands TLS.
fn check_endpoint(endpoint: &str, mode: TlsMode) -> Result<(), DriverError> {
    let lower = endpoint.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err(DriverError::configuration(format!(
            "endpoint_url must start with http:// or https://, got \"{endpoint}\""
        )));
    }
    if lower.starts_with("http://") && matches!(mode, TlsMode::Require | TlsMode::VerifyFull) {
        return Err(DriverError::new(
            ErrorKind::Tls,
            "TLS is required but endpoint_url uses plain http://",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::{ConnectionSettings, Secret};

    fn config(properties: &[(&str, &str)], username: Option<&str>) -> ConnectionConfig {
        let mut settings = ConnectionSettings {
            username: username.map(str::to_owned),
            ..ConnectionSettings::default()
        };
        for (name, value) in properties {
            settings
                .properties
                .insert((*name).to_owned(), (*value).to_owned());
        }
        ConnectionConfig::new(settings, Some(Secret::new("wJalrXUtnFEMI")))
    }

    #[tokio::test]
    async fn properties_set_region_and_endpoint() {
        let sdk = load_sdk_config(&config(
            &[
                ("region", "eu-central-1"),
                ("endpoint_url", "http://localhost:8000"),
            ],
            Some("AKIDEXAMPLE"),
        ))
        .await
        .unwrap();
        assert_eq!(sdk.region().map(Region::as_ref), Some("eu-central-1"));
        assert_eq!(sdk.endpoint_url(), Some("http://localhost:8000"));
        assert!(sdk.credentials_provider().is_some());
    }

    #[tokio::test]
    async fn access_keys_need_a_secret() {
        let mut without_secret = config(&[("region", "us-east-1")], Some("AKIDEXAMPLE"));
        without_secret.password = None;
        let error = load_sdk_config(&without_secret).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::Authentication);
    }

    #[test]
    fn endpoints_must_be_urls_and_honour_required_tls() {
        assert!(check_endpoint("localhost:8000", TlsMode::Prefer).is_err());
        assert!(check_endpoint("http://localhost:8000", TlsMode::Prefer).is_ok());
        let error = check_endpoint("http://localhost:8000", TlsMode::Require).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Tls);
        assert!(
            check_endpoint(
                "https://dynamodb.eu-west-1.amazonaws.com",
                TlsMode::VerifyFull
            )
            .is_ok()
        );
    }
}
