/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::ConnectionConfig;
use relix_driver_api::DriverError;
use relix_driver_http::Url;
use std::path::PathBuf;

const DEFAULT_ENDPOINT: &str = "https://bigquery.googleapis.com";
const API_PATH: &str = "bigquery/v2/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AuthMode {
    ApplicationDefault,
    ServiceAccountFile(PathBuf),
    Anonymous,
}

#[derive(Debug, Clone)]
pub(crate) struct BigQuerySettings {
    pub api_base: Url,
    pub auth: AuthMode,
    pub project: Option<String>,
    pub additional_projects: Vec<String>,
    pub location: Option<String>,
    pub dataset: Option<String>,
    pub maximum_bytes_billed: Option<u64>,
    pub job_timeout_ms: Option<u64>,
}

impl BigQuerySettings {
    pub(crate) fn from_config(config: &ConnectionConfig) -> Result<Self, DriverError> {
        let settings = &config.settings;
        let endpoint = settings.property("endpoint");
        let maximum_bytes_billed = settings
            .property("maximum_bytes_billed")
            .map(|value| {
                value.parse().map_err(|_| {
                    DriverError::configuration("\"maximum_bytes_billed\" must be a number of bytes")
                })
            })
            .transpose()?;
        Ok(Self {
            api_base: api_base(endpoint.unwrap_or(DEFAULT_ENDPOINT))?,
            auth: auth_mode(
                settings.property("auth"),
                settings.property("credentials_file"),
                endpoint.is_some(),
            )?,
            project: settings
                .property("project_id")
                .or(settings.database.as_deref())
                .map(str::trim)
                .filter(|project| !project.is_empty())
                .map(str::to_owned),
            additional_projects: settings
                .property("additional_projects")
                .map(|list| {
                    list.split(',')
                        .map(str::trim)
                        .filter(|project| !project.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            location: settings.property("location").map(str::to_owned),
            dataset: settings.property("dataset").map(str::to_owned),
            maximum_bytes_billed,
            job_timeout_ms: settings
                .query_timeout()
                .map(|timeout| u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)),
        })
    }
}

fn api_base(endpoint: &str) -> Result<Url, DriverError> {
    let root = format!("{}/", endpoint.trim().trim_end_matches('/'));
    Url::parse(&root)
        .and_then(|root| root.join(API_PATH))
        .map_err(|error| DriverError::configuration(format!("invalid BigQuery endpoint: {error}")))
}

/// Picks the credential source: an explicit service account key file, anonymous access
/// (only towards an overridden endpoint such as a local emulator) or Google's
/// Application Default Credentials chain.
fn auth_mode(
    auth: Option<&str>,
    credentials_file: Option<&str>,
    custom_endpoint: bool,
) -> Result<AuthMode, DriverError> {
    let normalized = auth.map(str::to_ascii_lowercase);
    match (normalized.as_deref(), credentials_file) {
        (Some("none"), _) if custom_endpoint => Ok(AuthMode::Anonymous),
        (Some("none"), _) => Err(DriverError::configuration(
            "anonymous access (auth = none) is only allowed with an \"endpoint\" override, \
             e.g. a local BigQuery emulator",
        )),
        (None | Some("service_account"), Some(path)) => {
            Ok(AuthMode::ServiceAccountFile(PathBuf::from(path)))
        }
        (Some("service_account"), None) => Err(DriverError::configuration(
            "service account authentication needs the \"credentials_file\" property",
        )),
        (None | Some("adc" | "application_default"), _) => Ok(AuthMode::ApplicationDefault),
        (Some(other), _) => Err(DriverError::configuration(format!(
            "unknown BigQuery auth mode \"{other}\" (use adc, service_account or none)"
        ))),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use relix_core::ConnectionSettings;

    fn config(properties: &[(&str, &str)]) -> ConnectionConfig {
        let mut settings = ConnectionSettings::default();
        for (name, value) in properties {
            settings
                .properties
                .insert((*name).to_owned(), (*value).to_owned());
        }
        ConnectionConfig::new(settings, None)
    }

    #[test]
    fn defaults_use_google_and_application_default_credentials() {
        let settings = BigQuerySettings::from_config(&config(&[("project_id", "acme")])).unwrap();
        assert_eq!(
            settings.api_base.as_str(),
            "https://bigquery.googleapis.com/bigquery/v2/"
        );
        assert_eq!(settings.auth, AuthMode::ApplicationDefault);
        assert_eq!(settings.project.as_deref(), Some("acme"));
    }

    #[test]
    fn emulator_endpoints_may_be_anonymous() {
        let settings = BigQuerySettings::from_config(&config(&[
            ("endpoint", "http://127.0.0.1:59050/"),
            ("auth", "none"),
            ("additional_projects", "bigquery-public-data, other"),
            ("maximum_bytes_billed", "1000000"),
        ]))
        .unwrap();
        assert_eq!(
            settings.api_base.as_str(),
            "http://127.0.0.1:59050/bigquery/v2/"
        );
        assert_eq!(settings.auth, AuthMode::Anonymous);
        assert_eq!(
            settings.additional_projects,
            vec!["bigquery-public-data", "other"]
        );
        assert_eq!(settings.maximum_bytes_billed, Some(1_000_000));
        assert!(BigQuerySettings::from_config(&config(&[("auth", "none")])).is_err());
    }

    #[test]
    fn credentials_files_select_service_accounts() {
        let settings =
            BigQuerySettings::from_config(&config(&[("credentials_file", "/keys/sa.json")]))
                .unwrap();
        assert_eq!(
            settings.auth,
            AuthMode::ServiceAccountFile("/keys/sa.json".into())
        );
        assert!(BigQuerySettings::from_config(&config(&[("auth", "service_account")])).is_err());
    }
}
