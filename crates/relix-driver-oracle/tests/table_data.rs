/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{CellChange, ChangeSet, RowChange, TableQuery, Value};
use relix_driver_api::ErrorKind;

fn ordered(limit: u32, offset: u64) -> TableQuery {
    TableQuery {
        tiebreaker: vec!["ID".into()],
        limit,
        offset,
        ..TableQuery::default()
    }
}

#[tokio::test]
async fn paginates_and_counts() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let name = common::create_items(connection.as_ref()).await;
    let table = common::table_ref(&config, &name);
    let first = sql.fetch_table_page(&table, &ordered(2, 0)).await.unwrap();
    assert_eq!(first.rows.len(), 2);
    assert!(first.has_more);
    assert_eq!(first.columns[0].name, "ID");
    let last = sql.fetch_table_page(&table, &ordered(2, 4)).await.unwrap();
    assert_eq!(
        last.rows,
        vec![vec![Value::Int(5), Value::Text("e".into())]]
    );
    assert!(!last.has_more);
    let unordered = sql
        .fetch_table_page(&table, &TableQuery::default())
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
    common::run(connection.as_ref(), &format!("DROP TABLE {name} PURGE")).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn applies_changes_in_one_transaction() {
    let Some(config) = common::config() else {
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
                    ("ID".into(), Value::Int(6)),
                    ("LABEL".into(), Value::Text("f".into())),
                ],
            },
            RowChange::Update {
                key: vec![("ID".into(), Value::Int(2))],
                cells: vec![CellChange {
                    column: "LABEL".into(),
                    original: Value::Text("b".into()),
                    updated: Value::Text("ß".into()),
                }],
            },
            RowChange::Delete {
                key: vec![("ID".into(), Value::Int(1))],
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
    common::run(connection.as_ref(), &format!("DROP TABLE {name} PURGE")).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn rolls_back_conflicting_changes() {
    let Some(config) = common::config() else {
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
                key: vec![("ID".into(), Value::Int(1))],
            },
            RowChange::Update {
                key: vec![("ID".into(), Value::Int(2))],
                cells: vec![CellChange {
                    column: "LABEL".into(),
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
                ("ID".into(), Value::Int(1)),
                ("LABEL".into(), Value::Text("dup".into())),
            ],
        }],
    };
    let error = sql.apply_changes(&duplicate).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 5);
    common::run(connection.as_ref(), &format!("DROP TABLE {name} PURGE")).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn edited_values_round_trip_through_their_text_form() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let name = common::object_name("typed");
    common::run(
        connection.as_ref(),
        &format!(
            "CREATE TABLE {name} (id NUMBER(10) PRIMARY KEY, at DATE NOT NULL, \
             at_ts TIMESTAMP(6) NOT NULL, at_tz TIMESTAMP(3) WITH TIME ZONE NOT NULL, \
             amount NUMBER(12, 4), ratio BINARY_DOUBLE, payload RAW(16), \
             note VARCHAR2(20) DEFAULT 'fresh')"
        ),
    )
    .await;
    common::run(
        connection.as_ref(),
        &format!(
            "INSERT INTO {name} VALUES (1, DATE '2026-10-23' + 8/24, \
             TIMESTAMP '2026-10-23 08:15:30.123456', \
             TIMESTAMP '2026-10-23 08:15:30.5 -05:30', 1234.5678, 0.1, HEXTORAW('CAFE'), NULL)"
        ),
    )
    .await;
    common::run(connection.as_ref(), "COMMIT").await;
    let table = common::table_ref(&config, &name);
    let page = sql.fetch_table_page(&table, &ordered(1, 0)).await.unwrap();
    let row = page.rows[0].clone();
    let cells = page
        .columns
        .iter()
        .zip(&row)
        .skip(1)
        .map(|(column, value)| CellChange {
            column: column.name.clone(),
            original: value.clone(),
            updated: if column.name == "NOTE" {
                Value::Text("edited".into())
            } else {
                value.clone()
            },
        })
        .collect();
    let changes = ChangeSet {
        table: table.clone(),
        changes: vec![RowChange::Update {
            key: vec![("ID".into(), row[0].clone())],
            cells,
        }],
    };
    let report = sql.apply_changes(&changes).await.unwrap();
    assert_eq!(report.updated, 1);
    let edited = sql.fetch_table_page(&table, &ordered(1, 0)).await.unwrap();
    assert_eq!(edited.rows[0][7], Value::Text("edited".into()));
    assert_eq!(edited.rows[0][..7], row[..7]);

    common::run(connection.as_ref(), &format!("DROP TABLE {name} PURGE")).await;

    let defaults_name = common::object_name("defaults");
    common::run(
        connection.as_ref(),
        &format!(
            "CREATE TABLE {defaults_name} (id NUMBER(10) GENERATED BY DEFAULT AS IDENTITY \
             PRIMARY KEY, note VARCHAR2(20) DEFAULT 'fresh')"
        ),
    )
    .await;
    let defaults_table = common::table_ref(&config, &defaults_name);
    let defaults = ChangeSet {
        table: defaults_table.clone(),
        changes: vec![RowChange::Insert { values: vec![] }],
    };
    assert_eq!(sql.apply_changes(&defaults).await.unwrap().inserted, 1);
    let inserted = sql
        .fetch_table_page(&defaults_table, &ordered(1, 0))
        .await
        .unwrap();
    assert_eq!(inserted.rows[0][1], Value::Text("fresh".into()));
    let name = defaults_name;
    common::run(connection.as_ref(), &format!("DROP TABLE {name} PURGE")).await;
    connection.close().await.unwrap();
}
