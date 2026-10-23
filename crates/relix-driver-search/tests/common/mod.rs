/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use relix_core::{
    CollectionRef, ConnectionConfig, ConnectionSettings, DocumentWrite, DocumentWriteOutcome,
    Secret, StatementOutcome, TlsMode, TlsSettings,
};
use relix_driver_api::{
    DatabaseConnection, DatabaseDriver, DocumentConnection, DriverError, ExecuteOptions,
    QueryExecutor,
};
use relix_driver_search::{ElasticsearchDriver, OpenSearchDriver};
use relix_testing::integration::integration_enabled;
use serde_json::Value as Json;
use std::env;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Product {
    Elasticsearch,
    OpenSearch,
}

impl Product {
    fn env_prefix(self) -> &'static str {
        match self {
            Self::Elasticsearch => "RELIX_TEST_ELASTICSEARCH",
            Self::OpenSearch => "RELIX_TEST_OPENSEARCH",
        }
    }

    fn default_port(self) -> u16 {
        match self {
            Self::Elasticsearch => 59200,
            Self::OpenSearch => 59201,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Elasticsearch => "Elasticsearch",
            Self::OpenSearch => "OpenSearch",
        }
    }

    pub fn driver(self) -> Box<dyn DatabaseDriver> {
        match self {
            Self::Elasticsearch => Box::new(ElasticsearchDriver::new()),
            Self::OpenSearch => Box::new(OpenSearchDriver::new()),
        }
    }

    pub fn other_driver(self) -> Box<dyn DatabaseDriver> {
        match self {
            Self::Elasticsearch => Box::new(OpenSearchDriver::new()),
            Self::OpenSearch => Box::new(ElasticsearchDriver::new()),
        }
    }

    pub fn has_security(self) -> bool {
        self == Self::Elasticsearch
    }
}

/// Connection settings for the search servers of `docker-compose.test.yml`: Elasticsearch
/// with security (user `elastic`) on 59200 and OpenSearch without security on 59201.
/// Returns `None` unless `RELIX_INTEGRATION=1` is set.
pub fn config(product: Product) -> Option<ConnectionConfig> {
    if !integration_enabled() {
        eprintln!("skipping {product:?} integration test: set RELIX_INTEGRATION=1");
        return None;
    }
    let read = |suffix: &str| env::var(format!("{}_{suffix}", product.env_prefix())).ok();
    let port = read("PORT")
        .and_then(|port| port.parse().ok())
        .unwrap_or(product.default_port());
    let default_user = product.has_security().then(|| "elastic".to_owned());
    let settings = ConnectionSettings {
        host: Some(read("HOST").unwrap_or_else(|| "127.0.0.1".to_owned())),
        port: Some(port),
        username: read("USER").or(default_user),
        connect_timeout_secs: 10,
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        ..ConnectionSettings::default()
    };
    let password =
        read("PASSWORD").or_else(|| product.has_security().then(|| "relix_test".to_owned()));
    Some(ConnectionConfig::new(settings, password.map(Secret::new)))
}

pub async fn connect_with(
    product: Product,
    config: &ConnectionConfig,
) -> Box<dyn DatabaseConnection> {
    product.driver().connect(config).await.expect("connect")
}

pub async fn connect(product: Product) -> Option<Box<dyn DatabaseConnection>> {
    let config = config(product)?;
    Some(connect_with(product, &config).await)
}

pub fn documents(connection: &dyn DatabaseConnection) -> &dyn DocumentConnection {
    connection.as_document().expect("document connection")
}

pub fn console(connection: &dyn DatabaseConnection) -> &dyn QueryExecutor {
    connection.as_query().expect("query executor")
}

pub async fn run(connection: &dyn DatabaseConnection, request: &str) -> StatementOutcome {
    try_run(connection, request)
        .await
        .unwrap_or_else(|error| panic!("{request}: {error}"))
}

pub async fn try_run(
    connection: &dyn DatabaseConnection,
    request: &str,
) -> Result<StatementOutcome, DriverError> {
    console(connection)
        .execute(request, &ExecuteOptions::default())
        .await
}

pub async fn write(
    connection: &dyn DatabaseConnection,
    collection: &CollectionRef,
    operation: DocumentWrite,
) -> Result<DocumentWriteOutcome, DriverError> {
    documents(connection).write(collection, &operation).await
}

pub async fn cluster(connection: &dyn DatabaseConnection) -> String {
    documents(connection).namespaces().await.unwrap()[0]
        .name
        .clone()
}

pub async fn collection(connection: &dyn DatabaseConnection, name: &str) -> CollectionRef {
    CollectionRef::new(cluster(connection).await, name)
}

pub async fn drop_quietly(connection: &dyn DatabaseConnection, path: &str) {
    let _ = try_run(connection, &format!("DELETE /{path}")).await;
}

pub fn rows(outcome: StatementOutcome) -> relix_core::RowBatch {
    match outcome {
        StatementOutcome::Rows(rows) => rows,
        other => panic!("expected rows, got {other:?}"),
    }
}

pub fn row_value(rows: &relix_core::RowBatch, key: &str) -> Option<relix_core::Value> {
    rows.rows
        .iter()
        .find(|row| row[0] == relix_core::Value::Text(key.to_owned()))
        .map(|row| row[1].clone())
}

pub fn key_id(key: &Json) -> String {
    key["_id"].as_str().expect("_id").to_owned()
}

#[macro_export]
macro_rules! for_each_product {
    ($scenario:ident) => {
        mod $scenario {
            #[tokio::test]
            async fn elasticsearch() {
                super::$scenario(super::common::Product::Elasticsearch).await;
            }

            #[tokio::test]
            async fn opensearch() {
                super::$scenario(super::common::Product::OpenSearch).await;
            }
        }
    };
}
