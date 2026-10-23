/*
 * @author t0bx
 * @since 24.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{
    CellChange, ChangeReport, ChangeSet, ObjectInfo, RowChange, SortDirection, SortKey, TableQuery,
    Value,
};
use relix_driver_api::{DatabaseConnection, ErrorKind};

async fn object(connection: &dyn DatabaseConnection, name: &str) -> ObjectInfo {
    common::sql(connection)
        .objects("neo4j", None)
        .await
        .unwrap()
        .into_iter()
        .find(|object| object.object.name == name)
        .unwrap()
}

fn query(filter: Option<&str>, offset: u64, limit: u32) -> TableQuery {
    TableQuery {
        filter: filter.map(str::to_owned),
        sort: vec![SortKey {
            column: "seq".into(),
            direction: SortDirection::Ascending,
        }],
        tiebreaker: vec!["elementId".into()],
        offset,
        limit,
    }
}

#[tokio::test]
async fn pages_nodes_and_relationships() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let label = common::label("Item");
    let link = common::label("NEXT");
    common::run(
        connection.as_ref(),
        &format!("UNWIND range(1, 25) AS i CREATE (:`{label}` {{seq: i, even: i % 2 = 0}})"),
    )
    .await;
    common::run(
        connection.as_ref(),
        &format!(
            "MATCH (a:`{label}`), (b:`{label}`) WHERE b.seq = a.seq + 1 AND a.seq <= 3 \
             CREATE (a)-[:`{link}` {{weight: a.seq}}]->(b)"
        ),
    )
    .await;
    let nodes = object(connection.as_ref(), &label).await;
    let first = sql
        .fetch_table_page(&nodes.object, &query(None, 0, 10))
        .await
        .unwrap();
    let names: Vec<&str> = first
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(names, ["elementId", "labels", "even", "seq"]);
    assert_eq!(first.rows.len(), 10);
    assert!(first.has_more);
    assert_eq!(first.rows[0][1], Value::Text(format!("[\"{label}\"]")));
    assert_eq!(first.rows[0][3], Value::Int(1));
    let last = sql
        .fetch_table_page(&nodes.object, &query(None, 20, 10))
        .await
        .unwrap();
    assert_eq!(last.rows.len(), 5);
    assert!(!last.has_more);
    assert_eq!(last.rows[0][3], Value::Int(21));
    let even = sql
        .fetch_table_page(&nodes.object, &query(Some("n.even"), 0, 50))
        .await
        .unwrap();
    assert_eq!(even.rows.len(), 12);
    assert_eq!(sql.count_rows(&nodes.object, None).await.unwrap(), 25);
    assert_eq!(
        sql.count_rows(&nodes.object, Some("n.seq > 20"))
            .await
            .unwrap(),
        5
    );

    let relationships = object(connection.as_ref(), &link).await;
    let page = sql
        .fetch_table_page(
            &relationships.object,
            &TableQuery {
                sort: vec![SortKey {
                    column: "weight".into(),
                    direction: SortDirection::Descending,
                }],
                ..TableQuery::default()
            },
        )
        .await
        .unwrap();
    let names: Vec<&str> = page
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "elementId",
            "type",
            "startElementId",
            "endElementId",
            "weight"
        ]
    );
    assert_eq!(page.rows.len(), 3);
    assert_eq!(page.rows[0][1], Value::Text(link.clone()));
    assert_eq!(page.rows[0][4], Value::Int(3));
    assert_eq!(
        sql.count_rows(&relationships.object, Some("a.seq = 1"))
            .await
            .unwrap(),
        1
    );
    let refused = sql
        .fetch_table_page(&nodes.object, &query(Some("n.seq >"), 0, 5))
        .await
        .unwrap_err();
    assert_eq!(refused.kind, ErrorKind::Query);
    common::remove_label(connection.as_ref(), &label).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn applies_property_changes_in_one_transaction() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let label = common::label("Account");
    common::run(
        connection.as_ref(),
        &format!(
            "UNWIND range(1, 3) AS i CREATE (:`{label}` {{seq: i, name: 'n' + i, balance: 10, \
             opened: date('2024-01-01')}})"
        ),
    )
    .await;
    let nodes = object(connection.as_ref(), &label).await;
    let page = sql
        .fetch_table_page(&nodes.object, &query(None, 0, 10))
        .await
        .unwrap();
    let id = |row: usize| vec![("elementId".to_owned(), page.rows[row][0].clone())];
    let balance = common::column(&page, "balance");
    let name = common::column(&page, "name");
    let changes = ChangeSet {
        table: nodes.object.clone(),
        changes: vec![
            RowChange::Update {
                key: id(0),
                cells: vec![
                    CellChange {
                        column: "balance".into(),
                        original: Value::Int(10),
                        updated: Value::Text("25".into()),
                    },
                    CellChange {
                        column: "nickname".into(),
                        original: Value::Null,
                        updated: Value::Text("first".into()),
                    },
                ],
            },
            RowChange::Delete { key: id(2) },
        ],
    };
    assert_eq!(
        sql.apply_changes(&changes).await.unwrap(),
        ChangeReport {
            updated: 1,
            inserted: 0,
            deleted: 1
        }
    );
    let check = common::rows(
        connection.as_ref(),
        &format!("MATCH (n:`{label}` {{seq: 1}}) RETURN n.balance AS balance, n.nickname AS nick"),
    )
    .await;
    assert_eq!(
        check.rows[0],
        vec![Value::Int(25), Value::Text("first".into())]
    );
    assert_eq!(sql.count_rows(&nodes.object, None).await.unwrap(), 2);

    let stale = ChangeSet {
        table: nodes.object.clone(),
        changes: vec![
            RowChange::Update {
                key: id(1),
                cells: vec![CellChange {
                    column: "name".into(),
                    original: page.rows[1][name].clone(),
                    updated: Value::Text("renamed".into()),
                }],
            },
            RowChange::Update {
                key: id(0),
                cells: vec![CellChange {
                    column: "balance".into(),
                    original: page.rows[0][balance].clone(),
                    updated: Value::Text("99".into()),
                }],
            },
        ],
    };
    let error = sql.apply_changes(&stale).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::ConcurrentModification, "{error}");
    let untouched = common::rows(
        connection.as_ref(),
        &format!("MATCH (n:`{label}` {{seq: 2}}) RETURN n.name AS name"),
    )
    .await;
    assert_eq!(untouched.rows[0][0], Value::Text("n2".into()));

    let wrong_type = ChangeSet {
        table: nodes.object.clone(),
        changes: vec![RowChange::Update {
            key: id(1),
            cells: vec![CellChange {
                column: "balance".into(),
                original: Value::Int(10),
                updated: Value::Text("lots".into()),
            }],
        }],
    };
    assert_eq!(
        sql.apply_changes(&wrong_type).await.unwrap_err().kind,
        ErrorKind::Query
    );
    let temporal = ChangeSet {
        table: nodes.object.clone(),
        changes: vec![RowChange::Update {
            key: id(1),
            cells: vec![CellChange {
                column: "opened".into(),
                original: Value::Text("2024-01-01".into()),
                updated: Value::Text("2025-01-01".into()),
            }],
        }],
    };
    assert_eq!(
        sql.apply_changes(&temporal).await.unwrap_err().kind,
        ErrorKind::Unsupported
    );
    let insert = ChangeSet {
        table: nodes.object.clone(),
        changes: vec![RowChange::Insert {
            values: vec![("name".into(), Value::Text("new".into()))],
        }],
    };
    assert_eq!(
        sql.apply_changes(&insert).await.unwrap_err().kind,
        ErrorKind::Unsupported
    );
    common::remove_label(connection.as_ref(), &label).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn deleting_a_connected_node_rolls_back() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let label = common::label("Hub");
    common::run(
        connection.as_ref(),
        &format!("CREATE (:`{label}` {{seq: 1}})-[:LINKS]->(:`{label}` {{seq: 2}})"),
    )
    .await;
    let nodes = object(connection.as_ref(), &label).await;
    let page = sql
        .fetch_table_page(&nodes.object, &query(None, 0, 10))
        .await
        .unwrap();
    let changes = ChangeSet {
        table: nodes.object.clone(),
        changes: vec![
            RowChange::Update {
                key: vec![("elementId".into(), page.rows[1][0].clone())],
                cells: vec![CellChange {
                    column: "seq".into(),
                    original: Value::Int(2),
                    updated: Value::Text("20".into()),
                }],
            },
            RowChange::Delete {
                key: vec![("elementId".into(), page.rows[0][0].clone())],
            },
        ],
    };
    let error = sql.apply_changes(&changes).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query, "{error}");
    let after = common::rows(
        connection.as_ref(),
        &format!("MATCH (n:`{label}`) RETURN n.seq AS seq ORDER BY seq"),
    )
    .await;
    assert_eq!(after.rows, vec![vec![Value::Int(1)], vec![Value::Int(2)]]);
    common::remove_label(connection.as_ref(), &label).await;
    connection.close().await.unwrap();
}
