/*
 * @author t0bx
 * @since 23.10.2026
 */

mod common;

use relix_core::{ChangeSet, RowChange, SortDirection, SortKey, TableQuery, TableRef, Value};
use relix_driver_api::{DatabaseDriver, ErrorKind};
use relix_driver_trino::TrinoDriver;

#[tokio::test]
async fn pages_are_filtered_sorted_and_flag_further_rows() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let nation = TableRef::new(Some(common::TPCH.into()), Some("tiny".into()), "nation");
    let query = TableQuery {
        filter: Some("regionkey > 0".into()),
        sort: vec![SortKey {
            column: "nationkey".into(),
            direction: SortDirection::Descending,
        }],
        tiebreaker: vec!["nationkey".into()],
        offset: 2,
        limit: 5,
    };
    let page = sql.fetch_table_page(&nation, &query).await.unwrap();
    assert_eq!(page.rows.len(), 5);
    assert!(page.has_more);
    assert_eq!(page.rows[0][0], Value::Int(22));
    let tail = sql
        .fetch_table_page(
            &nation,
            &TableQuery {
                offset: 18,
                ..query
            },
        )
        .await
        .unwrap();
    assert_eq!(tail.rows.len(), 2);
    assert!(!tail.has_more);
    assert_eq!(sql.count_rows(&nation, None).await.unwrap(), 25);
    assert_eq!(
        sql.count_rows(&nation, Some("regionkey > 0"))
            .await
            .unwrap(),
        20
    );

    let unqualified = TableRef::new(None, None, "region");
    assert_eq!(sql.count_rows(&unqualified, None).await.unwrap(), 5);

    let error = sql
        .apply_changes(&ChangeSet {
            table: nation,
            changes: vec![RowChange::Delete {
                key: vec![("nationkey".into(), Value::Int(1))],
            }],
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert!(!TrinoDriver::new().capabilities().table_editing);
    connection.close().await.unwrap();
}
