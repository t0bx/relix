/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::ObjectKind;

#[tokio::test]
async fn browses_keyspaces_tables_views_columns_and_indexes() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let keyspace = common::create_keyspace(connection.as_ref()).await;
    common::run(
        connection.as_ref(),
        &format!(
            "CREATE TABLE {keyspace}.orders (tenant int, day date, id timeuuid, plan text static, \
             total decimal, note text, PRIMARY KEY ((tenant, day), id)) \
             WITH CLUSTERING ORDER BY (id DESC) AND comment = 'customer orders'"
        ),
    )
    .await;
    common::run(
        connection.as_ref(),
        &format!("CREATE INDEX orders_note ON {keyspace}.orders (note)"),
    )
    .await;
    common::run(
        connection.as_ref(),
        &format!(
            "CREATE MATERIALIZED VIEW {keyspace}.orders_by_total AS SELECT tenant, day, id, total \
             FROM {keyspace}.orders WHERE tenant IS NOT NULL AND day IS NOT NULL AND id IS NOT NULL \
             AND total IS NOT NULL PRIMARY KEY (total, tenant, day, id)"
        ),
    )
    .await;

    let databases = sql.databases().await.unwrap();
    let names: Vec<&str> = databases
        .iter()
        .map(|database| database.name.as_str())
        .collect();
    assert!(names.contains(&keyspace.as_str()));
    assert!(names.contains(&"system_schema"));
    let first_system = names
        .iter()
        .position(|name| name.starts_with("system"))
        .unwrap();
    assert!(names[..first_system].contains(&keyspace.as_str()));
    assert!(sql.schemas(&keyspace).await.unwrap().is_empty());

    let objects = sql.objects(&keyspace, None).await.unwrap();
    let table = objects
        .iter()
        .find(|object| object.object.name == "orders")
        .unwrap();
    assert_eq!(table.kind, ObjectKind::Table);
    assert_eq!(table.comment.as_deref(), Some("customer orders"));
    let view = objects
        .iter()
        .find(|object| object.object.name == "orders_by_total")
        .unwrap();
    assert_eq!(view.kind, ObjectKind::MaterializedView);
    assert_eq!(view.comment.as_deref(), Some("view of orders"));
    assert_eq!(
        objects.len(),
        2,
        "index backing views must be hidden: {objects:?}"
    );

    let details = sql.table_details(&table.object).await.unwrap();
    let columns: Vec<(&str, &str, bool, Option<&str>)> = details
        .columns
        .iter()
        .map(|column| {
            (
                column.name.as_str(),
                column.data_type.as_str(),
                column.is_primary_key,
                column.comment.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        columns,
        vec![
            ("tenant", "int", true, Some("partition key #1")),
            ("day", "date", true, Some("partition key #2")),
            ("id", "timeuuid", true, Some("clustering column #1 (DESC)")),
            ("plan", "text", false, Some("static column")),
            ("note", "text", false, Some("regular column")),
            ("total", "decimal", false, Some("regular column")),
        ]
    );
    assert_eq!(details.primary_key, ["tenant", "day", "id"]);
    assert_eq!(details.row_identity().unwrap(), ["tenant", "day", "id"]);
    assert!(details.indexes[0].is_primary);
    let index = details
        .indexes
        .iter()
        .find(|index| index.name == "orders_note")
        .unwrap();
    assert_eq!(index.columns, ["note"]);
    assert!(details.foreign_keys.is_empty());
    let definition = details.definition.unwrap();
    assert!(definition.contains("CREATE TABLE"), "{definition}");

    let view_details = sql.table_details(&view.object).await.unwrap();
    assert_eq!(view_details.kind, ObjectKind::MaterializedView);
    assert_eq!(view_details.primary_key[0], "total");
    let view_definition = sql.object_definition(view).await.unwrap().unwrap();
    assert!(
        view_definition.contains("MATERIALIZED VIEW"),
        "{view_definition}"
    );

    let missing = relix_core::TableRef::new(Some(keyspace.clone()), None, "missing");
    assert!(sql.table_details(&missing).await.is_err());
    common::drop_keyspace(connection.as_ref(), &keyspace).await;
    connection.close().await.unwrap();
}
