/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::url_codec::{decode, encode};
use relix_core::ConnectionSettings;
use relix_driver_api::DriverError;
use relix_driver_http::RequestBuilder;
use relix_driver_http::header::HeaderMap;
use std::collections::BTreeMap;

const SCHEMA_PROPERTY: &str = "schema";
const SESSION_PROPERTIES_PROPERTY: &str = "session_properties";

/// Client-side session of the Trino protocol. The server keeps no session: every
/// request carries the catalog, schema, session properties, path, prepared statements
/// and transaction, and responses tell the client how to change them (`USE`, `SET
/// SESSION`, `PREPARE`, `START TRANSACTION`, …).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionState {
    pub(crate) catalog: Option<String>,
    pub(crate) schema: Option<String>,
    properties: BTreeMap<String, String>,
    path: Option<String>,
    prepared: BTreeMap<String, String>,
    transaction_id: Option<String>,
}

impl SessionState {
    pub(crate) fn from_settings(settings: &ConnectionSettings) -> Result<Self, DriverError> {
        let mut properties = BTreeMap::new();
        if let Some(list) = settings.property(SESSION_PROPERTIES_PROPERTY) {
            for entry in list
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
            {
                let (name, value) = entry.split_once('=').ok_or_else(|| {
                    DriverError::configuration(format!(
                        "session_properties entry \"{entry}\" is not name=value"
                    ))
                })?;
                properties.insert(name.trim().to_owned(), value.trim().to_owned());
            }
        }
        Ok(Self {
            catalog: settings
                .database
                .as_deref()
                .map(str::trim)
                .filter(|catalog| !catalog.is_empty())
                .map(str::to_owned),
            schema: settings.property(SCHEMA_PROPERTY).map(str::to_owned),
            properties,
            ..Self::default()
        })
    }

    pub(crate) fn apply(&self, mut request: RequestBuilder) -> RequestBuilder {
        if let Some(catalog) = &self.catalog {
            request = request.header("X-Trino-Catalog", catalog.as_str());
        }
        if let Some(schema) = &self.schema {
            request = request.header("X-Trino-Schema", schema.as_str());
        }
        if let Some(path) = &self.path {
            request = request.header("X-Trino-Path", path.as_str());
        }
        if let Some(transaction) = &self.transaction_id {
            request = request.header("X-Trino-Transaction-Id", transaction.as_str());
        }
        if !self.properties.is_empty() {
            request = request.header("X-Trino-Session", joined(&self.properties));
        }
        if !self.prepared.is_empty() {
            request = request.header("X-Trino-Prepared-Statement", joined(&self.prepared));
        }
        request
    }

    /// Applies the session changes a response announces in its `X-Trino-Set-*`,
    /// `X-Trino-Clear-*`, prepare and transaction headers.
    pub(crate) fn update(&mut self, headers: &HeaderMap) {
        let values = |name: &str| -> Vec<String> {
            headers
                .get_all(name)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .map(str::to_owned)
                .collect()
        };
        if let Some(catalog) = values("X-Trino-Set-Catalog").pop() {
            self.catalog = Some(catalog);
        }
        if let Some(schema) = values("X-Trino-Set-Schema").pop() {
            self.schema = Some(schema);
        }
        if let Some(path) = values("X-Trino-Set-Path").pop() {
            self.path = Some(path);
        }
        for entry in values("X-Trino-Set-Session") {
            if let Some((name, value)) = entry.split_once('=') {
                self.properties.insert(name.to_owned(), decode(value));
            }
        }
        for name in values("X-Trino-Clear-Session") {
            self.properties.remove(&name);
        }
        for entry in values("X-Trino-Added-Prepare") {
            if let Some((name, statement)) = entry.split_once('=') {
                self.prepared.insert(decode(name), decode(statement));
            }
        }
        for name in values("X-Trino-Deallocated-Prepare") {
            self.prepared.remove(&decode(&name));
        }
        if let Some(transaction) = values("X-Trino-Started-Transaction-Id").pop() {
            self.transaction_id = Some(transaction);
        }
        if !values("X-Trino-Clear-Transaction-Id").is_empty() {
            self.transaction_id = None;
        }
    }
}

fn joined(entries: &BTreeMap<String, String>) -> String {
    entries
        .iter()
        .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_driver_http::header::HeaderValue;

    #[test]
    fn response_headers_change_the_session() {
        let mut state = SessionState::default();
        let mut headers = HeaderMap::new();
        headers.insert("X-Trino-Set-Catalog", HeaderValue::from_static("tpch"));
        headers.insert("X-Trino-Set-Schema", HeaderValue::from_static("tiny"));
        headers.append(
            "X-Trino-Set-Session",
            HeaderValue::from_static("query_max_run_time=5m"),
        );
        headers.append(
            "X-Trino-Set-Session",
            HeaderValue::from_static("join_distribution_type=BROADCAST"),
        );
        headers.insert(
            "X-Trino-Added-Prepare",
            HeaderValue::from_static("q1=SELECT+%3F+FROM+nation"),
        );
        state.update(&headers);
        assert_eq!(state.catalog.as_deref(), Some("tpch"));
        assert_eq!(state.schema.as_deref(), Some("tiny"));
        assert_eq!(state.properties.len(), 2);
        assert_eq!(state.prepared["q1"], "SELECT ? FROM nation");

        let mut clearing = HeaderMap::new();
        clearing.insert(
            "X-Trino-Clear-Session",
            HeaderValue::from_static("query_max_run_time"),
        );
        clearing.insert(
            "X-Trino-Deallocated-Prepare",
            HeaderValue::from_static("q1"),
        );
        state.update(&clearing);
        assert_eq!(state.properties.len(), 1);
        assert!(state.prepared.is_empty());
    }

    #[test]
    fn settings_provide_the_initial_session() {
        let mut settings = ConnectionSettings {
            database: Some("tpch".into()),
            ..ConnectionSettings::default()
        };
        settings
            .properties
            .insert(SCHEMA_PROPERTY.into(), "tiny".into());
        settings
            .properties
            .insert(SESSION_PROPERTIES_PROPERTY.into(), "a=1, b = x y".into());
        let state = SessionState::from_settings(&settings).unwrap();
        assert_eq!(state.catalog.as_deref(), Some("tpch"));
        assert_eq!(state.schema.as_deref(), Some("tiny"));
        assert_eq!(joined(&state.properties), "a=1,b=x+y");
        settings
            .properties
            .insert(SESSION_PROPERTIES_PROPERTY.into(), "broken".into());
        assert!(SessionState::from_settings(&settings).is_err());
    }
}
