/*
 * @author t0bx
 * @since 24.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{ObjectKind, TableRef};

#[tokio::test]
async fn browses_databases_labels_relationship_types_and_schema() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let label = common::label("Person");
    let relationship = common::label("FOLLOWS");
    common::run(
        connection.as_ref(),
        &format!(
            "CREATE (a:`{label}` {{name: 'Ada', age: 36}})-[:`{relationship}` {{since: 2020}}]->\
             (:`{label}` {{name: 'Bob', age: 'unknown'}}), (:`{label}` {{name: 'Cy'}})"
        ),
    )
    .await;
    let index = format!("{label}_name");
    let constraint = format!("{label}_unique_name");
    common::run(
        connection.as_ref(),
        &format!("CREATE INDEX `{index}` FOR (n:`{label}`) ON (n.age)"),
    )
    .await;
    common::run(
        connection.as_ref(),
        &format!("CREATE CONSTRAINT `{constraint}` FOR (n:`{label}`) REQUIRE n.name IS UNIQUE"),
    )
    .await;

    let databases = sql.databases().await.unwrap();
    let default = databases
        .iter()
        .find(|database| database.is_default)
        .unwrap();
    assert_eq!(default.name, "neo4j");
    assert_eq!(databases.last().unwrap().name, "system");
    assert!(sql.schemas("neo4j").await.unwrap().is_empty());

    let objects = sql.objects("neo4j", None).await.unwrap();
    let node_label = objects
        .iter()
        .find(|object| object.object.name == label)
        .unwrap();
    assert_eq!(node_label.kind, ObjectKind::NodeLabel);
    assert_eq!(node_label.comment.as_deref(), Some("3 nodes"));
    let relationship_type = objects
        .iter()
        .find(|object| object.object.name == relationship)
        .unwrap();
    assert_eq!(relationship_type.kind, ObjectKind::RelationshipType);
    assert_eq!(relationship_type.comment.as_deref(), Some("1 relationship"));

    let details = sql.table_details(&node_label.object).await.unwrap();
    let columns: Vec<(&str, &str, bool)> = details
        .columns
        .iter()
        .map(|column| {
            (
                column.name.as_str(),
                column.data_type.as_str(),
                column.nullable,
            )
        })
        .collect();
    assert_eq!(
        columns,
        vec![
            ("elementId", "String", false),
            ("labels", "List<String>", false),
            ("age", "Long | String", true),
            ("name", "String", false),
        ]
    );
    assert_eq!(details.row_identity().unwrap(), ["elementId"]);
    let age_index = details
        .indexes
        .iter()
        .find(|item| item.name == index)
        .unwrap();
    assert_eq!(age_index.columns, ["age"]);
    assert!(!age_index.is_unique);
    let unique = details
        .indexes
        .iter()
        .find(|item| item.name == constraint)
        .unwrap();
    assert!(unique.is_unique);
    let definition = details.definition.unwrap();
    assert!(definition.contains("CREATE CONSTRAINT"), "{definition}");
    assert!(definition.contains("CREATE RANGE INDEX"), "{definition}");
    assert_eq!(
        sql.object_definition(node_label).await.unwrap().as_deref(),
        Some(definition.as_str())
    );

    let relationship_details = sql.table_details(&relationship_type.object).await.unwrap();
    let names: Vec<&str> = relationship_details
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "elementId",
            "type",
            "startElementId",
            "endElementId",
            "since"
        ]
    );

    let unqualified = TableRef::new(None, None, label.clone());
    assert_eq!(
        sql.table_details(&unqualified).await.unwrap().kind,
        ObjectKind::NodeLabel
    );
    assert!(
        sql.table_details(&TableRef::new(None, None, "NoSuchThing"))
            .await
            .is_err()
    );
    common::run(
        connection.as_ref(),
        &format!("DROP CONSTRAINT `{constraint}`"),
    )
    .await;
    common::run(connection.as_ref(), &format!("DROP INDEX `{index}`")).await;
    common::remove_label(connection.as_ref(), &label).await;
    connection.close().await.unwrap();
}
