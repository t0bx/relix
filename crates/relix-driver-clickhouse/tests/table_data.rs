/*
 * @author t0bx
 * @since 23.10.2026
 */

mod common;

use relix_core::{ChangeSet, RowChange, SortDirection, SortKey, TableQuery, TableRef, Value};
use relix_driver_api::{DatabaseDriver, ErrorKind};
use relix_driver_clickhouse::ClickHouseDriver;

#[tokio::test]
async fn pages_are_filtered_sorted_and_flag_further_rows() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let name = common::table_name("pages");
    common::run(
        connection.as_ref(),
        &format!("CREATE TABLE {name} (id UInt32, label String) ENGINE = MergeTree ORDER BY id"),
    )
    .await;
    common::run(
        connection.as_ref(),
        &format!("INSERT INTO {name} SELECT number + 1, concat('row ', toString(number + 1)) FROM numbers(25)"),
    )
    .await;
    let table = TableRef::new(Some(common::database(&config)), None, name.clone());
    let query = TableQuery {
        filter: Some("id > 4".into()),
        sort: vec![SortKey {
            column: "id".into(),
            direction: SortDirection::Descending,
        }],
        tiebreaker: vec!["id".into()],
        offset: 5,
        limit: 10,
    };
    let page = sql.fetch_table_page(&table, &query).await.unwrap();
    assert_eq!(page.rows.len(), 10);
    assert!(page.has_more);
    assert_eq!(
        page.rows[0],
        vec![Value::Int(20), Value::Text("row 20".into())]
    );
    let tail = sql
        .fetch_table_page(
            &table,
            &TableQuery {
                offset: 20,
                ..query
            },
        )
        .await
        .unwrap();
    assert_eq!(tail.rows.len(), 1);
    assert!(!tail.has_more);
    assert_eq!(sql.count_rows(&table, Some("id > 4")).await.unwrap(), 21);
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 25);

    let error = sql
        .apply_changes(&ChangeSet {
            table: table.clone(),
            changes: vec![RowChange::Delete {
                key: vec![("id".into(), Value::Int(1))],
            }],
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert!(!ClickHouseDriver::new().capabilities().table_editing);
    common::run(connection.as_ref(), &format!("DROP TABLE {name}")).await;
    connection.close().await.unwrap();
}
