/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Product, config, connect, connect_with, documents, rows, run};
use relix_core::{Secret, Value};
use relix_driver_api::ErrorKind;

async fn connects_and_identifies_the_product(product: Product) {
    let Some(connection) = connect(product).await else {
        return;
    };
    let info = connection.server_info();
    assert_eq!(info.product, product.name());
    assert!(
        info.version.starts_with(char::is_numeric),
        "{}",
        info.version
    );
    connection.ping().await.unwrap();
    assert!(connection.as_sql().is_none());
    let namespaces = documents(connection.as_ref()).namespaces().await.unwrap();
    assert_eq!(namespaces.len(), 1);
    assert!(namespaces[0].is_default);
    assert!(!namespaces[0].name.is_empty());
    connection.close().await.unwrap();
    assert_eq!(
        connection.ping().await.unwrap_err().kind,
        ErrorKind::Connection
    );
}
for_each_product!(connects_and_identifies_the_product);

async fn mismatched_driver_reports_the_real_product(product: Product) {
    let Some(config) = config(product) else {
        return;
    };
    let connection = product.other_driver().connect(&config).await.unwrap();
    assert_eq!(
        connection.server_info().product,
        format!(
            "{} (connected via {} driver)",
            product.name(),
            match product {
                Product::Elasticsearch => "OpenSearch",
                Product::OpenSearch => "Elasticsearch",
            }
        )
    );
}
for_each_product!(mismatched_driver_reports_the_real_product);

async fn unreachable_servers_are_connection_errors(product: Product) {
    let Some(mut config) = config(product) else {
        return;
    };
    config.settings.port = Some(1);
    config.settings.connect_timeout_secs = 3;
    let error = product.driver().connect(&config).await.err().unwrap();
    assert!(
        matches!(
            error.kind,
            ErrorKind::Connection | ErrorKind::Io | ErrorKind::Timeout
        ),
        "{error}"
    );
}
for_each_product!(unreachable_servers_are_connection_errors);

#[tokio::test]
async fn elasticsearch_wrong_password_is_an_authentication_error() {
    let Some(mut config) = config(Product::Elasticsearch) else {
        return;
    };
    config.password = Some(Secret::new("definitely-wrong"));
    let error = Product::Elasticsearch
        .driver()
        .connect(&config)
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication);
    assert!(!error.to_string().contains("definitely-wrong"));
}

#[tokio::test]
async fn elasticsearch_api_keys_authenticate() {
    let Some(config) = config(Product::Elasticsearch) else {
        return;
    };
    let admin = connect_with(Product::Elasticsearch, &config).await;
    let created = rows(
        run(
            admin.as_ref(),
            "POST /_security/api_key\n{\"name\": \"relix-test\", \"expiration\": \"1h\"}",
        )
        .await,
    );
    let field = |name: &str| match common::row_value(&created, name) {
        Some(Value::Text(text)) => text,
        other => panic!("{name}: {other:?}"),
    };
    let mut encoded = config.clone();
    encoded.settings.username = None;
    encoded.password = Some(Secret::new(field("encoded")));
    encoded
        .settings
        .properties
        .insert("auth".into(), "api_key".into());
    let by_encoded = connect_with(Product::Elasticsearch, &encoded).await;
    by_encoded.ping().await.unwrap();
    let mut split = encoded.clone();
    split.settings.username = Some(field("id"));
    split.password = Some(Secret::new(field("api_key")));
    let by_split = connect_with(Product::Elasticsearch, &split).await;
    let whoami = rows(run(by_split.as_ref(), "GET /_security/_authenticate").await);
    assert_eq!(
        common::row_value(&whoami, "authentication_type"),
        Some(Value::Text("api_key".into()))
    );
    split.password = Some(Secret::new("wrong"));
    let error = Product::Elasticsearch
        .driver()
        .connect(&split)
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication);
}
