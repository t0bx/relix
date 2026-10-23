/*
 * @author t0bx
 * @since 23.10.2026
 */

mod common;

use relix_core::{ObjectInfo, ObjectKind, TableRef};

#[tokio::test]
async fn databases_schemas_and_objects_are_listed() {
    let database = common::file_database();
    let connection = common::connect(&database.config).await;
    let sql = common::sql(connection.as_ref());
    common::run(connection.as_ref(), "ATTACH ':memory:' AS scratch").await;
    common::run(connection.as_ref(), "CREATE SCHEMA sales").await;
    common::run(
        connection.as_ref(),
        "CREATE TABLE sales.orders (id INTEGER PRIMARY KEY, total DECIMAL(10,2))",
    )
    .await;
    common::run(
        connection.as_ref(),
        "COMMENT ON TABLE sales.orders IS 'all orders'",
    )
    .await;
    common::run(
        connection.as_ref(),
        "CREATE VIEW sales.big_orders AS SELECT * FROM sales.orders WHERE total > 100",
    )
    .await;
    common::run(connection.as_ref(), "CREATE MACRO sales.net(x) AS x * 0.81").await;
    common::run(connection.as_ref(), "CREATE SEQUENCE sales.order_ids").await;

    let databases = sql.databases().await.unwrap();
    let names: Vec<_> = databases
        .iter()
        .map(|database| database.name.as_str())
        .collect();
    assert_eq!(names, [common::DATABASE, "scratch"]);
    assert!(databases[0].is_default);

    let schemas = sql.schemas(common::DATABASE).await.unwrap();
    assert_eq!(schemas, ["main", "sales"]);

    let objects = sql.objects(common::DATABASE, Some("sales")).await.unwrap();
    let listed: Vec<_> = objects
        .iter()
        .map(|object| (object.object.name.as_str(), object.kind))
        .collect();
    assert_eq!(
        listed,
        [
            ("orders", ObjectKind::Table),
            ("big_orders", ObjectKind::View),
            ("net", ObjectKind::Function),
            ("order_ids", ObjectKind::Sequence),
        ]
    );
    assert_eq!(objects[0].comment.as_deref(), Some("all orders"));
    assert_eq!(
        objects[0].object.database.as_deref(),
        Some(common::DATABASE)
    );

    for object in &objects {
        let definition = sql.object_definition(object).await.unwrap();
        assert!(
            definition
                .as_deref()
                .is_some_and(|text| text.starts_with("CREATE")),
            "{object:?} -> {definition:?}"
        );
    }
    connection.close().await.unwrap();
}

#[tokio::test]
async fn table_and_view_details_include_keys_and_definitions() {
    let database = common::file_database();
    let connection = common::connect(&database.config).await;
    let sql = common::sql(connection.as_ref());
    common::run(
        connection.as_ref(),
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, email VARCHAR NOT NULL UNIQUE)",
    )
    .await;
    common::run(
        connection.as_ref(),
        "CREATE TABLE orders (id INTEGER, customer_id INTEGER REFERENCES customers(id), \
         placed DATE DEFAULT current_date, PRIMARY KEY (id, customer_id))",
    )
    .await;
    common::run(
        connection.as_ref(),
        "CREATE INDEX orders_placed ON orders(placed)",
    )
    .await;
    common::run(
        connection.as_ref(),
        "CREATE VIEW recent AS SELECT * FROM orders WHERE placed > DATE '2024-01-01'",
    )
    .await;

    let table = TableRef::new(
        Some(common::DATABASE.into()),
        Some(common::SCHEMA.into()),
        "orders",
    );
    let details = sql.table_details(&table).await.unwrap();
    assert_eq!(details.kind, ObjectKind::Table);
    assert_eq!(details.primary_key, ["id", "customer_id"]);
    assert_eq!(
        details.row_identity(),
        Some(vec!["id".into(), "customer_id".into()])
    );
    let placed = &details.columns[2];
    assert_eq!(placed.data_type, "DATE");
    assert_eq!(placed.default_value.as_deref(), Some("current_date"));
    assert_eq!(details.foreign_keys.len(), 1);
    assert_eq!(details.foreign_keys[0].referenced_table.name, "customers");
    assert!(
        details
            .indexes
            .iter()
            .any(|index| index.name == "orders_placed"
                && index.columns == ["placed"]
                && !index.is_unique)
    );
    assert!(
        details
            .definition
            .as_deref()
            .unwrap()
            .starts_with("CREATE TABLE orders")
    );

    let customers = sql
        .table_details(&TableRef::new(None, None, "customers"))
        .await
        .unwrap();
    assert!(!customers.columns[1].nullable);
    assert!(
        customers
            .indexes
            .iter()
            .any(|index| index.is_unique && index.columns == ["email"])
    );

    let view = sql
        .table_details(&TableRef::new(None, Some(common::SCHEMA.into()), "recent"))
        .await
        .unwrap();
    assert_eq!(view.kind, ObjectKind::View);
    assert_eq!(view.columns.len(), 3);
    assert!(
        view.definition
            .as_deref()
            .unwrap()
            .starts_with("CREATE VIEW recent")
    );

    let missing = sql
        .object_definition(&ObjectInfo {
            object: TableRef::new(None, None, "absent"),
            kind: ObjectKind::Table,
            comment: None,
        })
        .await
        .unwrap();
    assert_eq!(missing, None);
    connection.close().await.unwrap();
}
