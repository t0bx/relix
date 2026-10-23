/*
 * @author t0bx
 * @since 23.10.2026
 */

mod common;

use relix_core::{ObjectKind, TableRef};

#[tokio::test]
async fn catalogs_schemas_and_objects_are_listed() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let catalogs = sql.databases().await.unwrap();
    assert!(
        catalogs
            .iter()
            .any(|catalog| catalog.name == common::TPCH && catalog.is_default)
    );
    assert!(
        catalogs
            .iter()
            .any(|catalog| catalog.name == common::MEMORY && !catalog.is_default)
    );

    let schemas = sql.schemas(common::TPCH).await.unwrap();
    assert!(schemas.iter().any(|schema| schema == "tiny"));
    assert!(schemas.iter().any(|schema| schema == "information_schema"));

    let objects = sql.objects(common::TPCH, Some("tiny")).await.unwrap();
    let nation = objects
        .iter()
        .find(|object| object.object.name == "nation")
        .unwrap();
    assert_eq!(nation.kind, ObjectKind::Table);
    assert_eq!(nation.object.database.as_deref(), Some(common::TPCH));
    assert_eq!(nation.object.schema.as_deref(), Some("tiny"));
    let definition = sql.object_definition(nation).await.unwrap().unwrap();
    assert!(
        definition.starts_with("CREATE TABLE tpch.tiny.nation"),
        "{definition}"
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn table_and_view_details_are_read() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let details = sql
        .table_details(&TableRef::new(
            Some(common::TPCH.into()),
            Some("tiny".into()),
            "nation",
        ))
        .await
        .unwrap();
    assert_eq!(details.kind, ObjectKind::Table);
    let names: Vec<_> = details
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(names, ["nationkey", "name", "regionkey", "comment"]);
    assert_eq!(details.columns[0].data_type, "bigint");
    assert!(details.primary_key.is_empty());
    assert!(details.definition.is_some());

    let view = common::table_name("recent");
    common::run(
        connection.as_ref(),
        &format!(
            "CREATE VIEW memory.default.{view} AS SELECT nationkey, name FROM tpch.tiny.nation"
        ),
    )
    .await;
    let objects = sql.objects(common::MEMORY, Some("default")).await.unwrap();
    let listed = objects
        .iter()
        .find(|object| object.object.name == view)
        .unwrap();
    assert_eq!(listed.kind, ObjectKind::View);
    let view_details = sql.table_details(&listed.object).await.unwrap();
    assert_eq!(view_details.kind, ObjectKind::View);
    assert_eq!(view_details.columns.len(), 2);
    assert!(
        view_details
            .definition
            .as_deref()
            .unwrap()
            .starts_with("CREATE VIEW")
    );
    common::run(
        connection.as_ref(),
        &format!("DROP VIEW memory.default.{view}"),
    )
    .await;
    connection.close().await.unwrap();
}
