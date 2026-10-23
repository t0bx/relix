/*
 * @author t0bx
 * @since 24.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use relix_core::{ChangeSet, ObjectInfo, ObjectKind, TableQuery, TableRef, Value};
use relix_driver_api::ErrorKind;
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path, query_param as query_matches};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn orders() -> TableRef {
    TableRef::new(Some(PROJECT.into()), Some("shop".into()), "orders")
}

async fn mount_orders_table(server: &MockServer, table_type: &str) {
    Mock::given(method("GET"))
        .and(path(api("datasets/shop/tables/orders")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "kind": "bigquery#table",
            "tableReference": {"projectId": PROJECT, "datasetId": "shop", "tableId": "orders"},
            "type": table_type,
            "numRows": "12",
            "schema": {"fields": [
                {"name": "id", "type": "INTEGER", "mode": "REQUIRED", "description": "order id"},
                {"name": "customer_id", "type": "INTEGER", "mode": "NULLABLE"},
                {"name": "tags", "type": "STRING", "mode": "REPEATED"},
                {"name": "status", "type": "STRING", "defaultValueExpression": "'new'"}
            ]},
            "view": {"query": "SELECT 1 AS id", "useLegacySql": false},
            "tableConstraints": {
                "primaryKey": {"columns": ["id"]},
                "foreignKeys": [{
                    "name": "fk_customer",
                    "referencedTable": {"projectId": PROJECT, "datasetId": "crm", "tableId": "customers"},
                    "columnReferences": [{"referencingColumn": "customer_id", "referencedColumn": "id"}]
                }]
            }
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn projects_datasets_and_objects_come_from_the_catalog_api() {
    let server = MockServer::start().await;
    let connection = connect(&server, &[("additional_projects", "bigquery-public-data")]).await;
    Mock::given(method("GET"))
        .and(path(api("datasets")))
        .and(query_matches("pageToken", "next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "datasets": [{"datasetReference": {"projectId": PROJECT, "datasetId": "crm"}}],
        })))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(api("datasets")))
        .and(query_matches("maxResults", "1000"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "datasets": [{"datasetReference": {"projectId": PROJECT, "datasetId": "shop"}}],
            "nextPageToken": "next",
        })))
        .with_priority(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(api("datasets/shop/tables")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tables": [
                {"tableReference": {"projectId": PROJECT, "datasetId": "shop", "tableId": "orders"}, "type": "TABLE"},
                {"tableReference": {"projectId": PROJECT, "datasetId": "shop", "tableId": "open_orders"}, "type": "VIEW"},
                {"tableReference": {"projectId": PROJECT, "datasetId": "shop", "tableId": "daily"}, "type": "MATERIALIZED_VIEW"}
            ],
            "totalItems": 3,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(api("datasets/shop/routines")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "routines": [
                {"routineReference": {"projectId": PROJECT, "datasetId": "shop", "routineId": "add_tax"}, "routineType": "SCALAR_FUNCTION"},
                {"routineReference": {"projectId": PROJECT, "datasetId": "shop", "routineId": "refresh"}, "routineType": "PROCEDURE"}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(api("datasets/crm/tables")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tables": [{"tableReference": {"projectId": PROJECT, "datasetId": "crm", "tableId": "customers"}, "type": "TABLE"}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(api("datasets/crm/routines")))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": {"code": 403, "message": "Permission bigquery.routines.list denied", "errors": [{"reason": "accessDenied"}]}
        })))
        .mount(&server)
        .await;

    let sql = connection.as_sql().unwrap();
    let databases = sql.databases().await.unwrap();
    assert_eq!(databases.len(), 2);
    assert!(databases[0].is_default && databases[0].name == PROJECT);
    assert_eq!(databases[1].name, "bigquery-public-data");
    assert_eq!(sql.schemas(PROJECT).await.unwrap(), vec!["shop", "crm"]);

    let objects = sql.objects(PROJECT, Some("shop")).await.unwrap();
    let summary: Vec<(&str, ObjectKind)> = objects
        .iter()
        .map(|object| (object.object.name.as_str(), object.kind))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("orders", ObjectKind::Table),
            ("open_orders", ObjectKind::View),
            ("daily", ObjectKind::MaterializedView),
            ("add_tax", ObjectKind::Function),
            ("refresh", ObjectKind::Procedure),
        ]
    );
    assert_eq!(objects[0].object, orders());
    let restricted = sql.objects(PROJECT, Some("crm")).await.unwrap();
    assert_eq!(
        restricted.len(),
        1,
        "tables stay visible without routine access"
    );
}

#[tokio::test]
async fn table_details_use_schema_and_constraints() {
    let server = MockServer::start().await;
    let connection = connect(&server, &[]).await;
    mount_orders_table(&server, "VIEW").await;
    let details = connection
        .as_sql()
        .unwrap()
        .table_details(&orders())
        .await
        .unwrap();
    assert_eq!(details.kind, ObjectKind::View);
    assert_eq!(details.primary_key, vec!["id"]);
    let id = &details.columns[0];
    assert!(id.is_primary_key && !id.nullable && id.data_type == "INT64");
    assert_eq!(id.comment.as_deref(), Some("order id"));
    assert_eq!(details.columns[2].data_type, "ARRAY<STRING>");
    assert_eq!(details.columns[3].default_value.as_deref(), Some("'new'"));
    let key = &details.foreign_keys[0];
    assert_eq!(key.columns, vec!["customer_id"]);
    assert_eq!(key.referenced_table.schema.as_deref(), Some("crm"));
    assert_eq!(details.definition.as_deref(), Some("SELECT 1 AS id"));
}

#[tokio::test]
async fn definitions_come_from_information_schema() {
    let server = MockServer::start().await;
    let connection = connect(&server, &[]).await;
    Mock::given(method("POST"))
        .and(path(api("queries")))
        .and(body_partial_json(json!({
            "query": "SELECT ddl FROM `acme`.`shop`.INFORMATION_SCHEMA.ROUTINES WHERE routine_name = 'add_tax'"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(query_response(
            "job_ddl",
            json!([field("ddl", "STRING")]),
            vec![row(&[json!("CREATE FUNCTION `acme.shop.add_tax`(x INT64) AS (x * 2);")])],
            1,
        )))
        .mount(&server)
        .await;
    let definition = connection
        .as_sql()
        .unwrap()
        .object_definition(&ObjectInfo {
            object: TableRef::new(Some(PROJECT.into()), Some("shop".into()), "add_tax"),
            kind: ObjectKind::Function,
            comment: None,
        })
        .await
        .unwrap();
    assert_eq!(
        definition.as_deref(),
        Some("CREATE FUNCTION `acme.shop.add_tax`(x INT64) AS (x * 2);")
    );
    let submit = body(&requests_to(&server, "POST", &api("queries")).await[0]);
    assert!(
        submit.get("createSession").is_none(),
        "browsing stays outside the session"
    );
}

#[tokio::test]
async fn plain_table_pages_read_stored_rows_without_a_query() {
    let server = MockServer::start().await;
    let connection = connect(&server, &[]).await;
    mount_orders_table(&server, "TABLE").await;
    Mock::given(method("GET"))
        .and(path(api("datasets/shop/tables/orders/data")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "kind": "bigquery#tableDataList",
            "totalRows": "12",
            "rows": [
                row(&[json!("5"), json!(null), json!([]), json!("new")]),
                row(&[json!("6"), json!("1"), json!([{"v": "x"}]), json!("paid")]),
                row(&[json!("7"), json!(null), json!([]), json!("new")]),
            ],
            "pageToken": "more",
        })))
        .mount(&server)
        .await;
    let page = connection
        .as_sql()
        .unwrap()
        .fetch_table_page(
            &orders(),
            &TableQuery {
                offset: 4,
                limit: 2,
                ..TableQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 2);
    assert!(page.has_more);
    assert_eq!(page.rows[1][2], Value::Text(r#"["x"]"#.into()));
    let reads = requests_to(&server, "GET", &api("datasets/shop/tables/orders/data")).await;
    assert_eq!(query_param(&reads[0], "startIndex").as_deref(), Some("4"));
    assert_eq!(query_param(&reads[0], "maxResults").as_deref(), Some("3"));
    assert!(
        requests_to(&server, "POST", &api("queries"))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn filtered_pages_and_counts_use_queries() {
    let server = MockServer::start().await;
    let connection = connect(&server, &[]).await;
    Mock::given(method("POST"))
        .and(path(api("queries")))
        .and(body_partial_json(json!({
            "query": "SELECT * FROM `acme`.`shop`.`orders` WHERE (status = 'new') LIMIT 3 OFFSET 0"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(query_response(
            "job_page",
            json!([field("id", "INTEGER")]),
            vec![row(&[json!("1")]), row(&[json!("2")])],
            2,
        )))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(api("queries")))
        .and(body_partial_json(json!({
            "query": "SELECT COUNT(*) FROM `acme`.`shop`.`orders` WHERE (status = 'new')"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(query_response(
            "job_count",
            json!([field("f0_", "INTEGER")]),
            vec![row(&[json!("2")])],
            1,
        )))
        .mount(&server)
        .await;
    let sql = connection.as_sql().unwrap();
    let page = sql
        .fetch_table_page(
            &orders(),
            &TableQuery {
                filter: Some("status = 'new'".into()),
                limit: 2,
                ..TableQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.rows, vec![vec![Value::Int(1)], vec![Value::Int(2)]]);
    assert!(!page.has_more);
    assert_eq!(
        sql.count_rows(&orders(), Some("status = 'new'"))
            .await
            .unwrap(),
        2
    );
    let error = sql
        .apply_changes(&ChangeSet {
            table: orders(),
            changes: Vec::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
}
