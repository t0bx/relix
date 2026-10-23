/*
 * @author t0bx
 * @since 23.10.2026
 */

mod common;

use relix_core::{ObjectKind, TableRef};

#[tokio::test]
async fn objects_details_and_definitions_are_listed() {
    let Some(config) = common::config() else {
        return;
    };
    let connection = common::connect(&config).await;
    let sql = common::sql(connection.as_ref());
    let database = common::table_name("relix_meta");
    let user = config.settings.username.clone().unwrap_or_default();
    let password = config
        .password
        .as_ref()
        .map(|secret| secret.expose().to_owned())
        .unwrap_or_default();
    for statement in [
        format!("CREATE DATABASE {database}"),
        format!(
            "CREATE TABLE {database}.events (id UInt64, kind LowCardinality(String), \
             note Nullable(String) COMMENT 'free text', created DateTime DEFAULT now(), \
             day Date MATERIALIZED toDate(created), \
             INDEX kind_idx kind TYPE set(10) GRANULARITY 1) \
             ENGINE = MergeTree ORDER BY (kind, id) COMMENT 'raw events'"
        ),
        format!("CREATE VIEW {database}.recent AS SELECT * FROM {database}.events WHERE id > 10"),
        format!(
            "CREATE MATERIALIZED VIEW {database}.per_kind ENGINE = SummingMergeTree ORDER BY kind \
             AS SELECT kind, count() AS total FROM {database}.events GROUP BY kind"
        ),
        format!(
            "CREATE DICTIONARY {database}.kinds (id UInt64, kind String) PRIMARY KEY id \
             SOURCE(CLICKHOUSE(DB '{database}' TABLE 'events' USER '{user}' PASSWORD '{password}')) \
             LIFETIME(0) LAYOUT(FLAT())"
        ),
    ] {
        common::run(connection.as_ref(), &statement).await;
    }

    let databases = sql.databases().await.unwrap();
    assert!(
        databases
            .iter()
            .any(|entry| entry.name == database && !entry.is_default)
    );
    assert!(sql.schemas(&database).await.unwrap().is_empty());

    let objects = sql.objects(&database, None).await.unwrap();
    let kinds: Vec<_> = objects
        .iter()
        .filter(|object| !object.object.name.starts_with(".inner"))
        .map(|object| (object.object.name.as_str(), object.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("events", ObjectKind::Table),
            ("kinds", ObjectKind::Table),
            ("per_kind", ObjectKind::MaterializedView),
            ("recent", ObjectKind::View),
        ]
    );
    let events = objects
        .iter()
        .find(|object| object.object.name == "events")
        .unwrap();
    assert_eq!(events.comment.as_deref(), Some("raw events"));

    let details = sql
        .table_details(&TableRef::new(Some(database.clone()), None, "events"))
        .await
        .unwrap();
    assert_eq!(details.kind, ObjectKind::Table);
    assert_eq!(details.primary_key, ["kind", "id"]);
    let note = &details.columns[2];
    assert!(note.nullable);
    assert_eq!(note.comment.as_deref(), Some("free text"));
    assert_eq!(details.columns[1].data_type, "LowCardinality(String)");
    assert!(!details.columns[1].nullable);
    assert_eq!(details.columns[3].default_value.as_deref(), Some("now()"));
    assert_eq!(
        details.columns[4].default_value.as_deref(),
        Some("MATERIALIZED toDate(created)")
    );
    assert!(details.indexes.iter().any(|index| index.name == "kind_idx"));
    assert!(details.foreign_keys.is_empty());
    assert!(
        details
            .definition
            .as_deref()
            .unwrap()
            .starts_with("CREATE TABLE")
    );

    for object in &objects {
        let definition = sql.object_definition(object).await.unwrap().unwrap();
        let expected = match object.object.name.as_str() {
            "kinds" => "CREATE DICTIONARY",
            "recent" => "CREATE VIEW",
            "per_kind" => "CREATE MATERIALIZED VIEW",
            _ => "CREATE TABLE",
        };
        assert!(definition.starts_with(expected), "{definition}");
    }
    common::run(connection.as_ref(), &format!("DROP DATABASE {database}")).await;
    connection.close().await.unwrap();
}
