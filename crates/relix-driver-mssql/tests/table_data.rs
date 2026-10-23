/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{CellChange, ChangeSet, RowChange, TableQuery, TableRef, Value};
use relix_driver_api::ErrorKind;

fn ordered(limit: u32, offset: u64) -> TableQuery {
    TableQuery {
        tiebreaker: vec!["id".into()],
        limit,
        offset,
        ..TableQuery::default()
    }
}

#[tokio::test]
async fn paginates_and_counts() {
    let Some(config) = common::config().await else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let name = common::create_items(connection.as_ref()).await;
    let table = common::table_ref(&config, &name);
    let first = sql.fetch_table_page(&table, &ordered(2, 0)).await.unwrap();
    assert_eq!(first.rows.len(), 2);
    assert!(first.has_more);
    assert_eq!(first.columns[0].name, "id");
    let last = sql.fetch_table_page(&table, &ordered(2, 4)).await.unwrap();
    assert_eq!(
        last.rows,
        vec![vec![Value::Int(5), Value::Text("e".into())]]
    );
    assert!(!last.has_more);
    let unordered = sql
        .fetch_table_page(
            &TableRef::new(None, None, name.clone()),
            &TableQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(unordered.rows.len(), 5);
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 5);
    assert_eq!(sql.count_rows(&table, Some("id > 3")).await.unwrap(), 2);
    let error = sql
        .count_rows(&table, Some("missing_column = 1"))
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 5);
    common::run(connection.as_ref(), &format!("DROP TABLE [{name}]")).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn applies_changes_in_one_transaction() {
    let Some(config) = common::config().await else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let name = common::create_items(connection.as_ref()).await;
    let table = common::table_ref(&config, &name);
    let changes = ChangeSet {
        table: table.clone(),
        changes: vec![
            RowChange::Insert {
                values: vec![
                    ("id".into(), Value::Int(6)),
                    ("label".into(), Value::Text("f".into())),
                ],
            },
            RowChange::Update {
                key: vec![("id".into(), Value::Int(2))],
                cells: vec![CellChange {
                    column: "label".into(),
                    original: Value::Text("b".into()),
                    updated: Value::Text("ß".into()),
                }],
            },
            RowChange::Delete {
                key: vec![("id".into(), Value::Int(1))],
            },
        ],
    };
    let report = sql.apply_changes(&changes).await.unwrap();
    assert_eq!((report.inserted, report.updated, report.deleted), (1, 1, 1));
    let rows = sql
        .fetch_table_page(&table, &ordered(10, 0))
        .await
        .unwrap()
        .rows;
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0], vec![Value::Int(2), Value::Text("ß".into())]);
    assert_eq!(rows[4], vec![Value::Int(6), Value::Text("f".into())]);
    common::run(connection.as_ref(), &format!("DROP TABLE [{name}]")).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn rolls_back_conflicting_changes() {
    let Some(config) = common::config().await else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let name = common::create_items(connection.as_ref()).await;
    let table = common::table_ref(&config, &name);
    let changes = ChangeSet {
        table: table.clone(),
        changes: vec![
            RowChange::Delete {
                key: vec![("id".into(), Value::Int(1))],
            },
            RowChange::Update {
                key: vec![("id".into(), Value::Int(2))],
                cells: vec![CellChange {
                    column: "label".into(),
                    original: Value::Text("stale".into()),
                    updated: Value::Text("z".into()),
                }],
            },
        ],
    };
    let error = sql.apply_changes(&changes).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::ConcurrentModification);
    assert!(error.message.contains("#2"));
    let rows = sql
        .fetch_table_page(&table, &ordered(10, 0))
        .await
        .unwrap()
        .rows;
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[1], vec![Value::Int(2), Value::Text("b".into())]);
    let duplicate = ChangeSet {
        table: table.clone(),
        changes: vec![RowChange::Insert {
            values: vec![
                ("id".into(), Value::Int(1)),
                ("label".into(), Value::Text("dup".into())),
            ],
        }],
    };
    let error = sql.apply_changes(&duplicate).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 5);
    common::run(connection.as_ref(), &format!("DROP TABLE [{name}]")).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn edited_values_round_trip_through_their_text_form() {
    let Some(config) = common::config().await else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let name = common::table_name("typed");
    common::run(
        connection.as_ref(),
        &format!(
            "CREATE TABLE [{name}] (id UNIQUEIDENTIFIER PRIMARY KEY, at DATETIME NOT NULL, \
             at2 DATETIME2 NOT NULL, offset_at DATETIMEOFFSET NOT NULL, amount DECIMAL(12, 4), \
             ratio FLOAT, flag BIT, payload VARBINARY(16), day DATE, note NVARCHAR(20))"
        ),
    )
    .await;
    common::run(
        connection.as_ref(),
        &format!(
            "INSERT INTO [{name}] VALUES (NEWID(), '2026-10-23T08:15:30.997', \
             '2026-10-23T08:15:30.1234567', '2026-10-23T08:15:30.5-05:30', 1234.5678, \
             0.1, 1, 0xCAFE, '2026-10-23', NULL)"
        ),
    )
    .await;
    let table = common::table_ref(&config, &name);
    let page = sql.fetch_table_page(&table, &ordered(1, 0)).await.unwrap();
    let row = &page.rows[0];
    let cells = page
        .columns
        .iter()
        .zip(row)
        .skip(1)
        .map(|(column, value)| CellChange {
            column: column.name.clone(),
            original: value.clone(),
            updated: if column.name == "note" {
                Value::Text("edited".into())
            } else {
                value.clone()
            },
        })
        .collect();
    let changes = ChangeSet {
        table: table.clone(),
        changes: vec![RowChange::Update {
            key: vec![("id".into(), row[0].clone())],
            cells,
        }],
    };
    let report = sql.apply_changes(&changes).await.unwrap();
    assert_eq!(report.updated, 1);
    let edited = sql.fetch_table_page(&table, &ordered(1, 0)).await.unwrap();
    assert_eq!(edited.rows[0][9], Value::Text("edited".into()));
    assert_eq!(edited.rows[0][..9], row[..9]);
    common::run(connection.as_ref(), &format!("DROP TABLE [{name}]")).await;
    connection.close().await.unwrap();
}
