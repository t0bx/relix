/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use relix_core::{ObjectKind, TableRef};

#[tokio::test]
async fn introspects_catalog_objects_and_definitions() {
    let Some(config) = common::config().await else {
        return;
    };
    let connection = common::connect(&config).await;
    let conn = connection.as_ref();
    let sql = common::sql(conn);
    let database = config.settings.database.clone().unwrap();
    let schema = common::table_name("app");
    let parent = common::table_name("parent");
    let child = common::table_name("child");
    let view = common::table_name("child_view");
    let procedure = common::table_name("touch");
    let function = common::table_name("twice");
    let trigger = common::table_name("audit");
    let sequence = common::table_name("seq");
    common::run(conn, &format!("CREATE SCHEMA [{schema}]")).await;
    common::run(
        conn,
        &format!(
            "CREATE TABLE [{schema}].[{parent}] (id INT CONSTRAINT [pk_{parent}] PRIMARY KEY)"
        ),
    )
    .await;
    common::run(
        conn,
        &format!(
            "CREATE TABLE [{schema}].[{child}] (\
               id BIGINT IDENTITY(10, 5) CONSTRAINT [pk_{child}] PRIMARY KEY, \
               parent_id INT NOT NULL, \
               code NVARCHAR(20) NOT NULL DEFAULT N'x', \
               amount DECIMAL(10, 2) NULL, \
               note NVARCHAR(MAX) NULL, \
               doubled AS (parent_id * 2), \
               CONSTRAINT [uq_{child}] UNIQUE (code), \
               CONSTRAINT [fk_{child}] FOREIGN KEY (parent_id) \
                 REFERENCES [{schema}].[{parent}] (id) ON DELETE CASCADE)"
        ),
    )
    .await;
    common::run(
        conn,
        &format!("CREATE INDEX [ix_{child}] ON [{schema}].[{child}] (parent_id DESC, amount)"),
    )
    .await;
    common::run(
        conn,
        &format!(
            "EXEC sp_addextendedproperty N'MS_Description', N'parents', N'SCHEMA', N'{schema}', \
             N'TABLE', N'{parent}'"
        ),
    )
    .await;
    common::run(
        conn,
        &format!(
            "EXEC sp_addextendedproperty N'MS_Description', N'the code', N'SCHEMA', N'{schema}', \
             N'TABLE', N'{child}', N'COLUMN', N'code'"
        ),
    )
    .await;
    common::run(
        conn,
        &format!("CREATE VIEW [{schema}].[{view}] AS SELECT id, code FROM [{schema}].[{child}]"),
    )
    .await;
    common::run(
        conn,
        &format!("CREATE PROCEDURE [{schema}].[{procedure}] AS SELECT 1 AS one"),
    )
    .await;
    common::run(
        conn,
        &format!(
            "CREATE FUNCTION [{schema}].[{function}] (@v INT) RETURNS INT AS BEGIN RETURN @v * 2 END"
        ),
    )
    .await;
    common::run(
        conn,
        &format!(
            "CREATE TRIGGER [{schema}].[{trigger}] ON [{schema}].[{parent}] AFTER INSERT AS \
             SET NOCOUNT ON"
        ),
    )
    .await;
    common::run(
        conn,
        &format!("CREATE SEQUENCE [{schema}].[{sequence}] AS INT START WITH 5 INCREMENT BY 2"),
    )
    .await;

    let databases = sql.databases().await.unwrap();
    assert!(
        databases
            .iter()
            .any(|entry| entry.name == database && entry.is_default)
    );
    assert!(databases.iter().any(|entry| entry.name == "master"));
    let schemas = sql.schemas(&database).await.unwrap();
    assert!(schemas.contains(&schema) && schemas.contains(&"dbo".to_owned()));
    assert!(
        !schemas
            .iter()
            .any(|name| name == "sys" || name == "db_owner")
    );

    let objects = sql.objects(&database, Some(&schema)).await.unwrap();
    let kind_of = |name: &str| {
        objects
            .iter()
            .find(|object| object.object.name == name)
            .map(|object| (object.kind, object.comment.clone()))
    };
    assert_eq!(
        kind_of(&parent),
        Some((ObjectKind::Table, Some("parents".into())))
    );
    assert_eq!(kind_of(&view), Some((ObjectKind::View, None)));
    assert_eq!(kind_of(&procedure), Some((ObjectKind::Procedure, None)));
    assert_eq!(kind_of(&function), Some((ObjectKind::Function, None)));
    assert_eq!(kind_of(&trigger), Some((ObjectKind::Trigger, None)));
    assert_eq!(kind_of(&sequence), Some((ObjectKind::Sequence, None)));
    assert!(
        objects
            .iter()
            .all(|object| object.object.schema.as_deref() == Some(schema.as_str()))
    );

    let details = sql
        .table_details(&TableRef::new(None, Some(schema.clone()), child.clone()))
        .await
        .unwrap();
    assert_eq!(
        details.table,
        TableRef::new(Some(database.clone()), Some(schema.clone()), child.clone())
    );
    assert_eq!(details.kind, ObjectKind::Table);
    assert_eq!(details.primary_key, vec!["id".to_owned()]);
    let names: Vec<&str> = details.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        ["id", "parent_id", "code", "amount", "note", "doubled"]
    );
    let id = &details.columns[0];
    assert!(id.is_primary_key && id.is_auto_increment && !id.nullable);
    assert_eq!(id.data_type, "bigint");
    let code = &details.columns[2];
    assert_eq!(code.data_type, "nvarchar(20)");
    assert_eq!(code.comment.as_deref(), Some("the code"));
    assert!(
        code.default_value
            .as_deref()
            .is_some_and(|d| d.contains('x'))
    );
    assert_eq!(details.columns[3].data_type, "decimal(10,2)");
    assert_eq!(details.columns[4].data_type, "nvarchar(max)");
    assert!(details.columns[4].nullable);
    assert!(details.indexes[0].is_primary);
    assert!(
        details
            .indexes
            .iter()
            .any(|index| index.name == format!("uq_{child}")
                && index.is_unique
                && !index.is_primary)
    );
    let secondary = details
        .indexes
        .iter()
        .find(|index| index.name == format!("ix_{child}"))
        .unwrap();
    assert_eq!(
        secondary.columns,
        vec!["parent_id".to_owned(), "amount".to_owned()]
    );
    let foreign_key = &details.foreign_keys[0];
    assert_eq!(foreign_key.name, format!("fk_{child}"));
    assert_eq!(foreign_key.columns, vec!["parent_id".to_owned()]);
    assert_eq!(
        foreign_key.referenced_table,
        TableRef::new(Some(database.clone()), Some(schema.clone()), parent.clone())
    );
    assert_eq!(foreign_key.referenced_columns, vec!["id".to_owned()]);
    let ddl = details.definition.unwrap();
    assert!(
        ddl.contains(&format!("CREATE TABLE [{schema}].[{child}]")),
        "{ddl}"
    );
    assert!(ddl.contains("[id] bigint IDENTITY(10,5) NOT NULL"), "{ddl}");
    assert!(ddl.contains("[doubled] AS ([parent_id]*(2))"), "{ddl}");
    assert!(ddl.contains("ON DELETE CASCADE"), "{ddl}");
    assert!(
        ddl.contains(&format!("CREATE NONCLUSTERED INDEX [ix_{child}]")),
        "{ddl}"
    );

    let view_details = sql
        .table_details(&TableRef::new(
            Some(database.clone()),
            Some(schema.clone()),
            view.clone(),
        ))
        .await
        .unwrap();
    assert_eq!(view_details.kind, ObjectKind::View);
    assert!(view_details.definition.unwrap().contains("CREATE VIEW"));

    let definition_of = |name: &str| {
        let object = objects
            .iter()
            .find(|object| object.object.name == name)
            .unwrap()
            .clone();
        async move { sql.object_definition(&object).await.unwrap().unwrap() }
    };
    assert!(definition_of(&procedure).await.contains("CREATE PROCEDURE"));
    assert!(definition_of(&function).await.contains("RETURN @v * 2"));
    assert!(definition_of(&trigger).await.contains("AFTER INSERT"));
    assert!(
        definition_of(&parent)
            .await
            .contains("PRIMARY KEY CLUSTERED")
    );
    let sequence_ddl = definition_of(&sequence).await;
    assert!(
        sequence_ddl.contains("AS int START WITH 5 INCREMENT BY 2"),
        "{sequence_ddl}"
    );

    for statement in [
        format!("DROP SEQUENCE [{schema}].[{sequence}]"),
        format!("DROP FUNCTION [{schema}].[{function}]"),
        format!("DROP PROCEDURE [{schema}].[{procedure}]"),
        format!("DROP VIEW [{schema}].[{view}]"),
        format!("DROP TABLE [{schema}].[{child}]"),
        format!("DROP TABLE [{schema}].[{parent}]"),
        format!("DROP SCHEMA [{schema}]"),
    ] {
        common::run(conn, &statement).await;
    }
    connection.close().await.unwrap();
}
