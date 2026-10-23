/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used)]

mod common;

use common::{Engine, for_each_engine};

use relix_core::{ObjectInfo, ObjectKind, TableRef};

for_each_engine!(
    lists_databases_and_schemas,
    browses_other_databases_on_the_same_server,
    describes_tables_views_and_routines,
);

async fn lists_databases_and_schemas(engine: Engine) {
    let Some(config) = engine.config() else {
        return;
    };
    let connection = common::connect(engine, &config).await;
    let sql = common::sql(connection.as_ref());
    let databases = sql.databases().await.unwrap();
    let default = databases
        .iter()
        .find(|database| database.is_default)
        .unwrap();
    assert_eq!(Some(&default.name), config.settings.database.as_ref());
    assert!(
        databases
            .iter()
            .all(|database| !database.name.starts_with("template"))
    );
    let schemas = sql.schemas(&default.name).await.unwrap();
    let public = schemas
        .iter()
        .position(|schema| schema == "public")
        .unwrap();
    let catalog = schemas
        .iter()
        .position(|schema| schema == "pg_catalog")
        .unwrap();
    assert!(public < catalog);
    let information_schema = schemas
        .iter()
        .position(|schema| schema == "information_schema")
        .unwrap();
    assert!(catalog < information_schema);
    assert!(!schemas.iter().any(|schema| schema == "pg_toast"));
    connection.close().await.unwrap();
}

async fn browses_other_databases_on_the_same_server(engine: Engine) {
    let Some(config) = engine.config() else {
        return;
    };
    let connection = common::connect(engine, &config).await;
    let sql = common::sql(connection.as_ref());
    let objects = sql.objects("postgres", Some("pg_catalog")).await.unwrap();
    let pg_class = objects
        .iter()
        .find(|object| object.object.name == "pg_class")
        .unwrap();
    assert_eq!(pg_class.object.database.as_deref(), Some("postgres"));
    let details = sql.table_details(&pg_class.object).await.unwrap();
    assert!(
        details
            .columns
            .iter()
            .any(|column| column.name == "relname")
    );
    connection.close().await.unwrap();
}

async fn describes_tables_views_and_routines(engine: Engine) {
    let Some(config) = engine.config() else {
        return;
    };
    let connection = common::connect(engine, &config).await;
    let sql = common::sql(connection.as_ref());
    let schema = common::table_name("intro");
    let setup = [
        format!("CREATE SCHEMA {schema}"),
        format!(
            "CREATE TABLE {schema}.parent (id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY, code text UNIQUE NOT NULL)"
        ),
        format!(
            "CREATE TABLE {schema}.child (id bigserial PRIMARY KEY, parent_id int NOT NULL REFERENCES {schema}.parent (id), \
             note varchar(40) DEFAULT 'n/a', amount numeric(10, 2))"
        ),
        format!("CREATE INDEX child_note_idx ON {schema}.child (note)"),
        format!("COMMENT ON TABLE {schema}.child IS 'child rows'"),
        format!("COMMENT ON COLUMN {schema}.child.note IS 'free text'"),
        format!("CREATE VIEW {schema}.child_view AS SELECT id, note FROM {schema}.child"),
        format!("CREATE MATERIALIZED VIEW {schema}.child_mat AS SELECT id FROM {schema}.child"),
        format!("CREATE SEQUENCE {schema}.counter START 5"),
        format!(
            "CREATE FUNCTION {schema}.twice(x int) RETURNS int LANGUAGE sql AS $$ SELECT x * 2 $$"
        ),
        format!(
            "CREATE FUNCTION {schema}.twice(x text) RETURNS text LANGUAGE sql AS $$ SELECT x || x $$"
        ),
        format!("CREATE PROCEDURE {schema}.noop() LANGUAGE sql AS $$ SELECT 1 $$"),
    ];
    for statement in &setup {
        common::run(connection.as_ref(), statement).await;
    }
    let database = config.settings.database.clone().unwrap();
    let objects = sql.objects(&database, Some(&schema)).await.unwrap();
    let kind_of = |name: &str| {
        objects
            .iter()
            .find(|object| object.object.name == name)
            .map(|object| object.kind)
    };
    assert_eq!(kind_of("parent"), Some(ObjectKind::Table));
    assert_eq!(kind_of("child_view"), Some(ObjectKind::View));
    assert_eq!(kind_of("child_mat"), Some(ObjectKind::MaterializedView));
    assert_eq!(kind_of("counter"), Some(ObjectKind::Sequence));
    assert_eq!(kind_of("twice"), Some(ObjectKind::Function));
    assert_eq!(kind_of("noop"), Some(ObjectKind::Procedure));
    let child_info = objects
        .iter()
        .find(|object| object.object.name == "child")
        .unwrap();
    assert_eq!(child_info.comment.as_deref(), Some("child rows"));
    assert_eq!(
        objects
            .iter()
            .filter(|object| object.object.name == "twice")
            .count(),
        1
    );

    let child = TableRef::new(Some(database.clone()), Some(schema.clone()), "child");
    let details = sql.table_details(&child).await.unwrap();
    assert_eq!(details.kind, ObjectKind::Table);
    assert_eq!(details.primary_key, vec!["id".to_owned()]);
    let id = &details.columns[0];
    assert!(id.is_primary_key && id.is_auto_increment && !id.nullable);
    let note = details
        .columns
        .iter()
        .find(|column| column.name == "note")
        .unwrap();
    assert_eq!(note.data_type, "character varying(40)");
    assert_eq!(note.comment.as_deref(), Some("free text"));
    assert!(note.default_value.as_deref().unwrap().contains("n/a"));
    assert!(
        details
            .indexes
            .iter()
            .any(|index| index.name == "child_note_idx"
                && index.columns == vec!["note".to_owned()]
                && !index.is_unique)
    );
    assert!(
        details
            .indexes
            .iter()
            .any(|index| index.is_primary && index.is_unique)
    );
    let foreign_key = &details.foreign_keys[0];
    assert_eq!(foreign_key.columns, vec!["parent_id".to_owned()]);
    assert_eq!(
        foreign_key.referenced_table,
        TableRef::new(Some(database.clone()), Some(schema.clone()), "parent")
    );
    assert_eq!(foreign_key.referenced_columns, vec!["id".to_owned()]);
    let ddl = details.definition.unwrap();
    assert!(
        ddl.starts_with(&format!(
            "CREATE TABLE {} (",
            engine.ddl_name(&schema, "child")
        )),
        "{ddl}"
    );
    assert!(ddl.contains("FOREIGN KEY (parent_id)"), "{ddl}");
    assert!(ddl.contains("INDEX child_note_idx"), "{ddl}");

    let parent = TableRef::new(Some(database.clone()), Some(schema.clone()), "parent");
    let parent_details = sql.table_details(&parent).await.unwrap();
    assert!(parent_details.columns[0].is_auto_increment);
    assert!(
        parent_details
            .definition
            .unwrap()
            .contains("GENERATED ALWAYS AS IDENTITY")
    );

    let object = |name: &str, kind| ObjectInfo {
        object: TableRef::new(Some(database.clone()), Some(schema.clone()), name),
        kind,
        comment: None,
    };
    let view = sql
        .object_definition(&object("child_view", ObjectKind::View))
        .await
        .unwrap()
        .unwrap();
    assert!(
        view.starts_with(&format!(
            "CREATE VIEW {} ",
            engine.ddl_name(&schema, "child_view")
        )),
        "{view}"
    );
    let materialized = sql
        .object_definition(&object("child_mat", ObjectKind::MaterializedView))
        .await
        .unwrap()
        .unwrap();
    assert!(
        materialized.starts_with("CREATE MATERIALIZED VIEW"),
        "{materialized}"
    );
    let functions = sql
        .object_definition(&object("twice", ObjectKind::Function))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(functions.matches(" FUNCTION ").count(), 2, "{functions}");
    assert!(functions.contains("\n\n"));
    let procedure = sql
        .object_definition(&object("noop", ObjectKind::Procedure))
        .await
        .unwrap()
        .unwrap();
    assert!(procedure.contains("PROCEDURE"), "{procedure}");
    let sequence = sql
        .object_definition(&object("counter", ObjectKind::Sequence))
        .await
        .unwrap()
        .unwrap();
    assert!(
        sequence.contains("START WITH 5") || sequence.contains("START 5"),
        "{sequence}"
    );
    let table_ddl = sql
        .object_definition(&object("parent", ObjectKind::Table))
        .await
        .unwrap()
        .unwrap();
    assert!(
        table_ddl.contains("UNIQUE") && table_ddl.contains("parent_code_key"),
        "{table_ddl}"
    );

    common::run(
        connection.as_ref(),
        &format!("DROP SCHEMA {schema} CASCADE"),
    )
    .await;
    connection.close().await.unwrap();
}
