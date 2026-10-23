/*
 * @author t0bx
 * @since 23.10.2026
 */

mod common;

use relix_core::{
    CellChange, ChangeSet, RowChange, SortDirection, SortKey, TableQuery, TableRef, Value,
};
use relix_driver_api::{DatabaseConnection, ErrorKind};

fn table(name: &str) -> TableRef {
    TableRef::new(
        Some(common::DATABASE.into()),
        Some(common::SCHEMA.into()),
        name,
    )
}

async fn seeded(connection: &dyn DatabaseConnection) {
    common::run(
        connection,
        "CREATE TABLE items (id INTEGER PRIMARY KEY, name VARCHAR, price DECIMAL(10,2), \
         ratio FLOAT, score DOUBLE, day DATE, seen TIMESTAMP, tags VARCHAR[], meta STRUCT(k INTEGER), \
         big HUGEINT, raw BLOB)",
    )
    .await;
    common::run(
        connection,
        "INSERT INTO items SELECT range, 'item ' || range, range * 1.25, range / 10, range / 3, \
         DATE '2024-01-01' + range::INTEGER, TIMESTAMP '2024-01-01 10:00:00.5' + to_days(range::INTEGER), \
         ['a b', 'c,' || range], {'k': range::INTEGER}, range::HUGEINT * 1000000000000000000000, \
         '\\x00\\xFF'::BLOB FROM range(1, 26)",
    )
    .await;
}

#[tokio::test]
async fn pages_are_filtered_sorted_and_flag_further_rows() {
    let database = common::file_database();
    let connection = common::connect(&database.config).await;
    let sql = common::sql(connection.as_ref());
    seeded(connection.as_ref()).await;
    let query = TableQuery {
        filter: Some("price > 5".into()),
        sort: vec![SortKey {
            column: "id".into(),
            direction: SortDirection::Descending,
        }],
        tiebreaker: vec!["id".into()],
        offset: 5,
        limit: 10,
    };
    let page = sql.fetch_table_page(&table("items"), &query).await.unwrap();
    assert_eq!(page.rows.len(), 10);
    assert!(page.has_more);
    assert_eq!(page.rows[0][0], Value::Int(20));
    let last = TableQuery {
        offset: 20,
        ..query.clone()
    };
    let tail = sql.fetch_table_page(&table("items"), &last).await.unwrap();
    assert_eq!(tail.rows.len(), 1);
    assert!(!tail.has_more);
    assert_eq!(
        sql.count_rows(&table("items"), Some("price > 5"))
            .await
            .unwrap(),
        21
    );
    assert_eq!(sql.count_rows(&table("items"), None).await.unwrap(), 25);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn edits_round_trip_every_displayed_type() {
    let database = common::file_database();
    let connection = common::connect(&database.config).await;
    let sql = common::sql(connection.as_ref());
    seeded(connection.as_ref()).await;
    let page = sql
        .fetch_table_page(&table("items"), &TableQuery::default())
        .await
        .unwrap();
    let first = &page.rows[0];
    for (index, column) in page.columns.iter().enumerate().skip(1) {
        let change = RowChange::Update {
            key: vec![("id".into(), first[0].clone())],
            cells: vec![CellChange {
                column: column.name.clone(),
                original: first[index].clone(),
                updated: first[index].clone(),
            }],
        };
        let report = sql
            .apply_changes(&ChangeSet {
                table: table("items"),
                changes: vec![change],
            })
            .await
            .unwrap_or_else(|error| panic!("{} ({:?}): {error}", column.name, first[index]));
        assert_eq!(report.updated, 1);
    }
    connection.close().await.unwrap();
}

#[tokio::test]
async fn change_sets_commit_together() {
    let database = common::file_database();
    let connection = common::connect(&database.config).await;
    let sql = common::sql(connection.as_ref());
    seeded(connection.as_ref()).await;
    let report = sql
        .apply_changes(&ChangeSet {
            table: table("items"),
            changes: vec![
                RowChange::Insert {
                    values: vec![
                        ("id".into(), Value::Int(100)),
                        ("name".into(), Value::Text("new".into())),
                        ("day".into(), Value::Text("2025-05-05".into())),
                    ],
                },
                RowChange::Update {
                    key: vec![("id".into(), Value::Int(1))],
                    cells: vec![CellChange {
                        column: "name".into(),
                        original: Value::Text("item 1".into()),
                        updated: Value::Text("renamed".into()),
                    }],
                },
                RowChange::Delete {
                    key: vec![("id".into(), Value::Int(2))],
                },
            ],
        })
        .await
        .unwrap();
    assert_eq!((report.inserted, report.updated, report.deleted), (1, 1, 1));
    let check = common::rows(
        connection.as_ref(),
        "SELECT (SELECT name FROM items WHERE id = 1), (SELECT day FROM items WHERE id = 100), \
         (SELECT count(*) FROM items WHERE id = 2)",
    )
    .await;
    assert_eq!(
        check.rows,
        vec![vec![
            Value::Text("renamed".into()),
            Value::Text("2025-05-05".into()),
            Value::Int(0)
        ]]
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn conflicting_changes_roll_back_the_whole_set() {
    let database = common::file_database();
    let connection = common::connect(&database.config).await;
    let sql = common::sql(connection.as_ref());
    seeded(connection.as_ref()).await;
    let error = sql
        .apply_changes(&ChangeSet {
            table: table("items"),
            changes: vec![
                RowChange::Delete {
                    key: vec![("id".into(), Value::Int(3))],
                },
                RowChange::Update {
                    key: vec![("id".into(), Value::Int(4))],
                    cells: vec![CellChange {
                        column: "name".into(),
                        original: Value::Text("stale value".into()),
                        updated: Value::Text("lost".into()),
                    }],
                },
            ],
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::ConcurrentModification);
    assert_eq!(
        sql.count_rows(&table("items"), Some("id = 3"))
            .await
            .unwrap(),
        1
    );

    common::run(connection.as_ref(), "BEGIN").await;
    common::run(
        connection.as_ref(),
        "UPDATE items SET name = 'editor' WHERE id = 5",
    )
    .await;
    let conflict = sql
        .apply_changes(&ChangeSet {
            table: table("items"),
            changes: vec![RowChange::Update {
                key: vec![("id".into(), Value::Int(5))],
                cells: vec![CellChange {
                    column: "name".into(),
                    original: Value::Text("item 5".into()),
                    updated: Value::Text("grid".into()),
                }],
            }],
        })
        .await
        .unwrap_err();
    assert_eq!(conflict.kind, ErrorKind::ConcurrentModification);
    common::run(connection.as_ref(), "ROLLBACK").await;
    connection.close().await.unwrap();
}
