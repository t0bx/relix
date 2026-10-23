/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use relix_core::{ChangeSet, ObjectInfo, ObjectKind, TableQuery, TableRef, Value};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, ErrorKind};
use relix_driver_snowflake::SnowflakeDriver;
use serde_json::{Value as Json, json};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn answer(server: &MockServer, statement: &str, columns: &[&str], rows: Json) {
    let row_type: Vec<Json> = columns.iter().map(|name| column(name, "text")).collect();
    let count = rows.as_array().map_or(0, Vec::len) as u64;
    Mock::given(method("POST"))
        .and(path("/api/v2/statements"))
        .and(body_partial_json(json!({ "statement": statement })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(result_set(row_type, rows, count, 1)),
        )
        .mount(server)
        .await;
}

async fn connect(server: &MockServer) -> Box<dyn DatabaseConnection> {
    answer(
        server,
        "SELECT CURRENT_VERSION()",
        &["CURRENT_VERSION()"],
        json!([["8.40.1"]]),
    )
    .await;
    SnowflakeDriver::new()
        .connect(&key_pair_config(server))
        .await
        .unwrap()
}

fn orders() -> TableRef {
    TableRef::new(Some("SALES".into()), Some("PUBLIC".into()), "ORDERS")
}

#[tokio::test]
async fn databases_schemas_and_objects_come_from_show_commands() {
    let server = MockServer::start().await;
    let connection = connect(&server).await;
    answer(
        &server,
        "SHOW DATABASES",
        &["created_on", "name", "is_default", "is_current", "owner"],
        json!([
            ["1", "HR", "Y", "N", "SYSADMIN"],
            ["1", "SALES", "N", "Y", "SYSADMIN"]
        ]),
    )
    .await;
    answer(
        &server,
        "SHOW TERSE SCHEMAS IN DATABASE \"SALES\"",
        &["created_on", "name", "kind", "database_name", "schema_name"],
        json!([
            ["1", "INFORMATION_SCHEMA", null, "SALES", null],
            ["1", "PUBLIC", null, "SALES", null]
        ]),
    )
    .await;
    let scope = "IN SCHEMA \"SALES\".\"PUBLIC\"";
    answer(
        &server,
        &format!("SHOW TERSE OBJECTS {scope}"),
        &["created_on", "name", "kind", "database_name", "schema_name"],
        json!([
            ["1", "ORDERS", "TABLE", "SALES", "PUBLIC"],
            ["1", "OPEN_ORDERS", "VIEW", "SALES", "PUBLIC"],
            ["1", "DAILY", "MATERIALIZED_VIEW", "SALES", "PUBLIC"]
        ]),
    )
    .await;
    answer(
        &server,
        &format!("SHOW USER FUNCTIONS {scope}"),
        &[
            "created_on",
            "name",
            "schema_name",
            "is_builtin",
            "arguments",
        ],
        json!([
            [
                "1",
                "ADD_TAX",
                "PUBLIC",
                "N",
                "ADD_TAX(NUMBER) RETURN NUMBER"
            ],
            [
                "1",
                "ADD_TAX",
                "PUBLIC",
                "N",
                "ADD_TAX(NUMBER, NUMBER) RETURN NUMBER"
            ]
        ]),
    )
    .await;
    answer(
        &server,
        &format!("SHOW PROCEDURES {scope}"),
        &[
            "created_on",
            "name",
            "schema_name",
            "is_builtin",
            "arguments",
        ],
        json!([
            ["1", "SYSTEM$X", "", "Y", "SYSTEM$X() RETURN VARCHAR"],
            ["1", "REFRESH", "PUBLIC", "N", "REFRESH() RETURN VARCHAR"]
        ]),
    )
    .await;
    answer(
        &server,
        &format!("SHOW SEQUENCES {scope}"),
        &["name", "database_name", "schema_name"],
        json!([["ORDER_SEQ", "SALES", "PUBLIC"]]),
    )
    .await;

    let sql = connection.as_sql().unwrap();
    let databases = sql.databases().await.unwrap();
    assert_eq!(databases.len(), 2);
    assert!(databases[1].is_default && !databases[0].is_default);
    assert_eq!(
        sql.schemas("SALES").await.unwrap(),
        vec!["INFORMATION_SCHEMA", "PUBLIC"]
    );
    let objects = sql.objects("SALES", Some("PUBLIC")).await.unwrap();
    let summary: Vec<(&str, ObjectKind)> = objects
        .iter()
        .map(|object| (object.object.name.as_str(), object.kind))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("ORDERS", ObjectKind::Table),
            ("OPEN_ORDERS", ObjectKind::View),
            ("DAILY", ObjectKind::MaterializedView),
            ("ADD_TAX", ObjectKind::Function),
            ("REFRESH", ObjectKind::Procedure),
            ("ORDER_SEQ", ObjectKind::Sequence),
        ]
    );
    assert_eq!(objects[0].object, orders());
}

#[tokio::test]
async fn table_details_combine_describe_and_imported_keys() {
    let server = MockServer::start().await;
    let connection = connect(&server).await;
    answer(
        &server,
        "SHOW TERSE OBJECTS LIKE 'ORDERS' IN SCHEMA \"SALES\".\"PUBLIC\"",
        &["created_on", "name", "kind", "database_name", "schema_name"],
        json!([["1", "ORDERS", "TABLE", "SALES", "PUBLIC"]]),
    )
    .await;
    answer(
        &server,
        "DESC TABLE \"SALES\".\"PUBLIC\".\"ORDERS\"",
        &[
            "name",
            "type",
            "kind",
            "null?",
            "default",
            "primary key",
            "unique key",
            "check",
            "expression",
            "comment",
        ],
        json!([
            [
                "ID",
                "NUMBER(38,0)",
                "COLUMN",
                "N",
                "IDENTITY START 1 INCREMENT 1 ORDER",
                "Y",
                "N",
                null,
                null,
                null
            ],
            [
                "CUSTOMER_ID",
                "NUMBER(38,0)",
                "COLUMN",
                "Y",
                null,
                "N",
                "N",
                null,
                null,
                "buyer"
            ],
            [
                "REGION",
                "VARCHAR(16777216)",
                "COLUMN",
                "Y",
                null,
                "N",
                "N",
                null,
                null,
                null
            ]
        ]),
    )
    .await;
    answer(
        &server,
        "SHOW IMPORTED KEYS IN TABLE \"SALES\".\"PUBLIC\".\"ORDERS\"",
        &[
            "pk_database_name",
            "pk_schema_name",
            "pk_table_name",
            "pk_column_name",
            "fk_column_name",
            "key_sequence",
            "fk_name",
        ],
        json!([
            [
                "SALES",
                "PUBLIC",
                "CUSTOMERS",
                "REGION",
                "REGION",
                "2",
                "FK_CUSTOMER"
            ],
            [
                "SALES",
                "PUBLIC",
                "CUSTOMERS",
                "ID",
                "CUSTOMER_ID",
                "1",
                "FK_CUSTOMER"
            ]
        ]),
    )
    .await;
    answer(
        &server,
        "SELECT GET_DDL('TABLE', '\"SALES\".\"PUBLIC\".\"ORDERS\"')",
        &["GET_DDL"],
        json!([["create or replace TABLE ORDERS (ID NUMBER(38,0));"]]),
    )
    .await;

    let sql = connection.as_sql().unwrap();
    let details = sql.table_details(&orders()).await.unwrap();
    assert_eq!(details.kind, ObjectKind::Table);
    assert_eq!(details.primary_key, vec!["ID"]);
    assert_eq!(details.columns.len(), 3);
    assert!(details.columns[0].is_auto_increment && !details.columns[0].nullable);
    assert_eq!(details.columns[1].comment.as_deref(), Some("buyer"));
    assert_eq!(details.columns[2].ordinal, 3);
    assert_eq!(details.foreign_keys.len(), 1);
    let key = &details.foreign_keys[0];
    assert_eq!(key.columns, vec!["CUSTOMER_ID", "REGION"]);
    assert_eq!(key.referenced_columns, vec!["ID", "REGION"]);
    assert_eq!(key.referenced_table.name, "CUSTOMERS");

    let definition = sql
        .object_definition(&ObjectInfo {
            object: orders(),
            kind: ObjectKind::Table,
            comment: None,
        })
        .await
        .unwrap();
    assert_eq!(
        definition.as_deref(),
        Some("create or replace TABLE ORDERS (ID NUMBER(38,0));")
    );
}

#[tokio::test]
async fn function_definitions_are_fetched_per_overload() {
    let server = MockServer::start().await;
    let connection = connect(&server).await;
    answer(
        &server,
        "SHOW USER FUNCTIONS LIKE 'ADD_TAX' IN SCHEMA \"SALES\".\"PUBLIC\"",
        &["name", "arguments"],
        json!([
            ["ADD_TAX", "ADD_TAX(NUMBER) RETURN NUMBER"],
            ["ADD_TAX", "ADD_TAX(NUMBER, [NUMBER]) RETURN NUMBER"]
        ]),
    )
    .await;
    for (signature, ddl) in [("(NUMBER)", "one"), ("(NUMBER, NUMBER)", "two")] {
        answer(
            &server,
            &format!("SELECT GET_DDL('FUNCTION', '\"SALES\".\"PUBLIC\".\"ADD_TAX\"{signature}')"),
            &["GET_DDL"],
            json!([[ddl]]),
        )
        .await;
    }
    let definition = connection
        .as_sql()
        .unwrap()
        .object_definition(&ObjectInfo {
            object: TableRef::new(Some("SALES".into()), Some("PUBLIC".into()), "ADD_TAX"),
            kind: ObjectKind::Function,
            comment: None,
        })
        .await
        .unwrap();
    assert_eq!(definition.as_deref(), Some("one\n\ntwo"));
}

#[tokio::test]
async fn table_pages_use_the_builder_and_detect_more_rows() {
    let server = MockServer::start().await;
    let connection = connect(&server).await;
    answer(
        &server,
        "SELECT * FROM \"SALES\".\"PUBLIC\".\"ORDERS\" ORDER BY \"ID\" LIMIT 3 OFFSET 4",
        &["ID"],
        json!([["5"], ["6"], ["7"]]),
    )
    .await;
    answer(
        &server,
        "SELECT COUNT(*) FROM \"SALES\".\"PUBLIC\".\"ORDERS\" WHERE (ID > 1)",
        &["COUNT(*)"],
        json!([["41"]]),
    )
    .await;
    let sql = connection.as_sql().unwrap();
    let page = sql
        .fetch_table_page(
            &orders(),
            &TableQuery {
                tiebreaker: vec!["ID".into()],
                offset: 4,
                limit: 2,
                ..TableQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        page.rows,
        vec![vec![Value::Text("5".into())], vec![Value::Text("6".into())]]
    );
    assert!(page.has_more);
    assert_eq!(sql.count_rows(&orders(), Some("ID > 1")).await.unwrap(), 41);
    let error = sql
        .apply_changes(&ChangeSet {
            table: orders(),
            changes: Vec::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
}
