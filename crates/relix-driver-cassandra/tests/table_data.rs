/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{
    CellChange, ChangeReport, ChangeSet, RowChange, SortDirection, SortKey, TableQuery, Value,
};
use relix_driver_api::{DatabaseConnection, ErrorKind};

async fn fixture(connection: &dyn DatabaseConnection) -> String {
    let keyspace = common::create_keyspace(connection).await;
    common::run(
        connection,
        &format!(
            "CREATE TABLE {keyspace}.items (bucket int, seq int, label text, flag boolean, \
             PRIMARY KEY (bucket, seq))"
        ),
    )
    .await;
    for seq in 1..=25 {
        common::run(
            connection,
            &format!(
                "INSERT INTO {keyspace}.items (bucket, seq, label, flag) VALUES (1, {seq}, 'row {seq}', {})",
                seq % 2 == 0
            ),
        )
        .await;
    }
    keyspace
}

fn query(filter: Option<&str>, offset: u64, limit: u32) -> TableQuery {
    TableQuery {
        filter: filter.map(str::to_owned),
        sort: vec![],
        tiebreaker: vec!["bucket".into(), "seq".into()],
        offset,
        limit,
    }
}

#[tokio::test]
async fn pages_skip_rows_without_offset_support() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let keyspace = fixture(connection.as_ref()).await;
    let table = common::table_ref(&keyspace, "items");
    let first = sql.fetch_table_page(&table, &query(None, 0, 10)).await.unwrap();
    assert_eq!(first.rows.len(), 10);
    assert!(first.has_more);
    assert_eq!(
        first.rows[0],
        vec![
            Value::Int(1),
            Value::Int(1),
            Value::Bool(false),
            Value::Text("row 1".into())
        ]
    );
    let last = sql.fetch_table_page(&table, &query(None, 20, 10)).await.unwrap();
    assert_eq!(last.rows.len(), 5);
    assert!(!last.has_more);
    assert_eq!(last.rows[0][1], Value::Int(21));

    let mut sorted = query(Some("bucket = 1"), 0, 3);
    sorted.sort = vec![SortKey {
        column: "seq".into(),
        direction: SortDirection::Descending,
    }];
    let descending = sql.fetch_table_page(&table, &sorted).await.unwrap();
    assert_eq!(descending.rows[0][1], Value::Int(25));

    let filtered = query(Some("flag = true ALLOW FILTERING"), 0, 50);
    assert_eq!(sql.fetch_table_page(&table, &filtered).await.unwrap().rows.len(), 12);
    let refused = sql
        .fetch_table_page(&table, &query(Some("flag = true"), 0, 50))
        .await
        .unwrap_err();
    assert_eq!(refused.kind, ErrorKind::Query, "{refused}");
    assert!(refused.message.to_uppercase().contains("ALLOW FILTERING"), "{refused}");

    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 25);
    assert_eq!(
        sql.count_rows(&table, Some("flag = true ALLOW FILTERING"))
            .await
            .unwrap(),
        12
    );
    common::drop_keyspace(connection.as_ref(), &keyspace).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn applies_conditional_changes_statement_by_statement() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let keyspace = fixture(connection.as_ref()).await;
    let table = common::table_ref(&keyspace, "items");
    let key = |seq: i64| vec![("bucket".to_owned(), Value::Int(1)), ("seq".to_owned(), Value::Int(seq))];
    let changes = ChangeSet {
        table: table.clone(),
        changes: vec![
            RowChange::Update {
                key: key(1),
                cells: vec![CellChange {
                    column: "label".into(),
                    original: Value::Text("row 1".into()),
                    updated: Value::Text("first".into()),
                }],
            },
            RowChange::Insert {
                values: vec![
                    ("bucket".into(), Value::Int(1)),
                    ("seq".into(), Value::Int(100)),
                    ("label".into(), Value::Text("new".into())),
                    ("flag".into(), Value::Null),
                ],
            },
            RowChange::Delete { key: key(2) },
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
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 25);
    let page = sql
        .fetch_table_page(&table, &query(Some("bucket = 1 AND seq = 1"), 0, 5))
        .await
        .unwrap();
    assert_eq!(page.rows[0][3], Value::Text("first".into()));

    let stale = ChangeSet {
        table: table.clone(),
        changes: vec![
            RowChange::Update {
                key: key(3),
                cells: vec![CellChange {
                    column: "flag".into(),
                    original: Value::Bool(false),
                    updated: Value::Bool(true),
                }],
            },
            RowChange::Update {
                key: key(1),
                cells: vec![CellChange {
                    column: "label".into(),
                    original: Value::Text("row 1".into()),
                    updated: Value::Text("lost update".into()),
                }],
            },
        ],
    };
    let error = sql.apply_changes(&stale).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::ConcurrentModification, "{error}");
    assert!(error.message.starts_with("change #2 (update)"), "{error}");
    assert!(error.detail.as_deref().unwrap().contains("1 earlier change(s)"));
    let saved = sql
        .fetch_table_page(&table, &query(Some("bucket = 1 AND seq = 3"), 0, 5))
        .await
        .unwrap();
    assert_eq!(saved.rows[0][2], Value::Bool(true));

    let duplicate = ChangeSet {
        table: table.clone(),
        changes: vec![RowChange::Insert {
            values: vec![("bucket".into(), Value::Int(1)), ("seq".into(), Value::Int(100))],
        }],
    };
    let error = sql.apply_changes(&duplicate).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::ConcurrentModification);
    let gone = ChangeSet {
        table: table.clone(),
        changes: vec![RowChange::Delete { key: key(2) }],
    };
    let error = sql.apply_changes(&gone).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::ConcurrentModification);
    let partial_key = ChangeSet {
        table: table.clone(),
        changes: vec![RowChange::Delete {
            key: vec![("bucket".into(), Value::Int(1))],
        }],
    };
    let error = sql.apply_changes(&partial_key).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::AmbiguousRow);
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 25);
    common::drop_keyspace(connection.as_ref(), &keyspace).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn counter_tables_are_not_edited_in_the_grid() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let keyspace = common::create_keyspace(connection.as_ref()).await;
    common::run(
        connection.as_ref(),
        &format!("CREATE TABLE {keyspace}.hits (page text PRIMARY KEY, views counter)"),
    )
    .await;
    common::run(
        connection.as_ref(),
        &format!("UPDATE {keyspace}.hits SET views = views + 3 WHERE page = 'home'"),
    )
    .await;
    let table = common::table_ref(&keyspace, "hits");
    let page = sql.fetch_table_page(&table, &query(None, 0, 10)).await.unwrap();
    assert_eq!(page.rows[0], vec![Value::Text("home".into()), Value::Int(3)]);
    let changes = ChangeSet {
        table,
        changes: vec![RowChange::Delete {
            key: vec![("page".into(), Value::Text("home".into()))],
        }],
    };
    let error = sql.apply_changes(&changes).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    common::drop_keyspace(connection.as_ref(), &keyspace).await;
    connection.close().await.unwrap();
}
