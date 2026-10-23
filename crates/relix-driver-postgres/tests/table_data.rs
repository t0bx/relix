/*
 * @author t0bx
 * @since 23.10.2026
 */

mod common;

use relix_core::{
    CellChange, ChangeReport, ChangeSet, RowChange, SortDirection, SortKey, TableQuery, Value,
};
use relix_driver_api::ErrorKind;

#[tokio::test]
async fn pages_through_rows_with_lookahead() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let name = common::table_name("page");
    common::run(
        connection.as_ref(),
        &format!(
            "CREATE TABLE {name} AS SELECT g AS id, g % 2 = 0 AS even, 'row ' || g AS label \
             FROM generate_series(1, 25) AS g"
        ),
    )
    .await;
    let table = common::table_ref(&config, &name);
    let mut query = TableQuery {
        filter: None,
        sort: vec![SortKey {
            column: "id".into(),
            direction: SortDirection::Descending,
        }],
        tiebreaker: vec![],
        offset: 0,
        limit: 10,
    };
    let first = sql.fetch_table_page(&table, &query).await.unwrap();
    assert_eq!(first.rows.len(), 10);
    assert!(first.has_more);
    assert_eq!(
        first.rows[0],
        vec![
            Value::Int(25),
            Value::Bool(false),
            Value::Text("row 25".into())
        ]
    );
    query.offset = 20;
    let last = sql.fetch_table_page(&table, &query).await.unwrap();
    assert_eq!(last.rows.len(), 5);
    assert!(!last.has_more);
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 25);
    assert_eq!(sql.count_rows(&table, Some("even")).await.unwrap(), 12);
    common::run(connection.as_ref(), &format!("DROP TABLE {name}")).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn applies_inserts_updates_and_deletes_in_one_transaction() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let name = common::table_name("changes");
    common::run(
        connection.as_ref(),
        &format!("CREATE TABLE {name} (id int PRIMARY KEY, label text, flag boolean)"),
    )
    .await;
    common::run(
        connection.as_ref(),
        &format!("INSERT INTO {name} VALUES (1, 'one', true), (2, 'two', false)"),
    )
    .await;
    let table = common::table_ref(&config, &name);
    let changes = ChangeSet {
        table: table.clone(),
        changes: vec![
            RowChange::Insert {
                values: vec![
                    ("id".into(), Value::Int(3)),
                    ("label".into(), Value::Text("it's three".into())),
                ],
            },
            RowChange::Update {
                key: vec![("id".into(), Value::Int(1))],
                cells: vec![CellChange {
                    column: "label".into(),
                    original: Value::Text("one".into()),
                    updated: Value::Text("uno".into()),
                }],
            },
            RowChange::Delete {
                key: vec![("id".into(), Value::Int(2))],
            },
        ],
    };
    let report = sql.apply_changes(&changes).await.unwrap();
    assert_eq!(
        report,
        ChangeReport {
            updated: 1,
            inserted: 1,
            deleted: 1
        }
    );
    let page = sql
        .fetch_table_page(
            &table,
            &TableQuery {
                tiebreaker: vec!["id".into()],
                ..TableQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        page.rows,
        vec![
            vec![Value::Int(1), Value::Text("uno".into()), Value::Bool(true)],
            vec![Value::Int(3), Value::Text("it's three".into()), Value::Null],
        ]
    );
    common::run(connection.as_ref(), &format!("DROP TABLE {name}")).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn conflicting_change_rolls_back_everything() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let name = common::table_name("conflict");
    common::run(
        connection.as_ref(),
        &format!("CREATE TABLE {name} (id int PRIMARY KEY, label text)"),
    )
    .await;
    common::run(
        connection.as_ref(),
        &format!("INSERT INTO {name} VALUES (1, 'current')"),
    )
    .await;
    let table = common::table_ref(&config, &name);
    let changes = ChangeSet {
        table: table.clone(),
        changes: vec![
            RowChange::Insert {
                values: vec![("id".into(), Value::Int(2))],
            },
            RowChange::Update {
                key: vec![("id".into(), Value::Int(1))],
                cells: vec![CellChange {
                    column: "label".into(),
                    original: Value::Text("stale".into()),
                    updated: Value::Text("mine".into()),
                }],
            },
        ],
    };
    let error = sql.apply_changes(&changes).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::ConcurrentModification);
    assert!(error.message.contains("change #2"), "{error}");
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 1);

    let duplicate = ChangeSet {
        table: table.clone(),
        changes: vec![RowChange::Insert {
            values: vec![("id".into(), Value::Int(1))],
        }],
    };
    let error = sql.apply_changes(&duplicate).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    assert!(error.detail.is_some(), "{error:?}");
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 1);
    common::run(connection.as_ref(), &format!("DROP TABLE {name}")).await;
    connection.close().await.unwrap();
}
