/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::flavor::Flavor;
use crate::introspection::timescale;
use crate::row_reader::read_column;
use relix_core::{DatabaseInfo, ObjectInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;
use tokio_postgres::Client;

macro_rules! visible_schema_condition {
    () => {
        "n.nspname <> 'pg_toast' AND n.nspname NOT LIKE 'pg\\_temp\\_%' \
         AND n.nspname NOT LIKE 'pg\\_toast\\_temp\\_%'"
    };
}

const DATABASES_SQL: &str = "SELECT datname::text FROM pg_catalog.pg_database \
     WHERE NOT datistemplate AND datallowconn ORDER BY datname";

const SCHEMAS_SQL: &str = concat!(
    "SELECT n.nspname::text FROM pg_catalog.pg_namespace n WHERE ",
    visible_schema_condition!(),
    " ORDER BY n.nspname"
);

const OBJECTS_SQL: &str = concat!(
    "SELECT n.nspname::text, c.relname::text, c.relkind::text, \
            obj_description(c.oid, 'pg_class') \
     FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE c.relkind IN ('r', 'p', 'f', 'v', 'm', 'S') \
       AND ($1::text IS NULL OR n.nspname = $1::text) AND ",
    visible_schema_condition!(),
    " UNION ALL \
     SELECT * FROM ( \
       SELECT DISTINCT ON (n.nspname, p.proname, p.prokind) \
              n.nspname::text, p.proname::text, \
              CASE p.prokind WHEN 'p' THEN 'procedure' ELSE 'function' END, \
              obj_description(p.oid, 'pg_proc') \
       FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
       WHERE p.prokind IN ('f', 'p') \
         AND ($1::text IS NULL OR n.nspname = $1::text) AND ",
    visible_schema_condition!(),
    " ORDER BY n.nspname, p.proname, p.prokind, p.oid \
     ) routines \
     ORDER BY 1, 2"
);

pub(crate) async fn list_databases(
    client: &Client,
    default_database: &str,
) -> Result<Vec<DatabaseInfo>, DriverError> {
    let rows = client.query(DATABASES_SQL, &[]).await.map_err(map_error)?;
    rows.iter()
        .map(|row| {
            let name: String = read_column(row, 0)?;
            Ok(DatabaseInfo {
                is_default: name == default_database,
                name,
            })
        })
        .collect()
}

pub(crate) async fn list_schemas(
    client: &Client,
    flavor: Flavor,
) -> Result<Vec<String>, DriverError> {
    let rows = client.query(SCHEMAS_SQL, &[]).await.map_err(map_error)?;
    let schemas = rows
        .iter()
        .map(|row| read_column(row, 0))
        .collect::<Result<Vec<String>, _>>()?;
    Ok(order_schemas(schemas, flavor.system_schemas()))
}

pub(crate) async fn list_objects(
    client: &Client,
    flavor: Flavor,
    database: &str,
    schema: Option<&str>,
) -> Result<Vec<ObjectInfo>, DriverError> {
    let rows = client
        .query(OBJECTS_SQL, &[&schema])
        .await
        .map_err(map_error)?;
    let mut objects = Vec::with_capacity(rows.len());
    for row in &rows {
        let kind_code: String = read_column(row, 2)?;
        let Some(kind) = object_kind(&kind_code) else {
            continue;
        };
        objects.push(ObjectInfo {
            object: TableRef::new(
                Some(database.to_owned()),
                Some(read_column(row, 0)?),
                read_column::<String>(row, 1)?,
            ),
            kind,
            comment: read_column(row, 3)?,
        });
    }
    if flavor.may_have_timescale_catalog()
        && let Some(timescale) = timescale::load_catalog(client, schema).await?
    {
        timescale.annotate(&mut objects);
    }
    Ok(objects)
}

pub(crate) fn order_schemas(mut schemas: Vec<String>, system_schemas: &[&str]) -> Vec<String> {
    schemas.sort_by(|left, right| {
        system_rank(left, system_schemas)
            .cmp(&system_rank(right, system_schemas))
            .then_with(|| left.cmp(right))
    });
    schemas
}

fn system_rank(schema: &str, system_schemas: &[&str]) -> usize {
    system_schemas
        .iter()
        .position(|system| *system == schema)
        .map_or(0, |position| position + 1)
}

fn object_kind(code: &str) -> Option<ObjectKind> {
    match code {
        "r" | "p" | "f" => Some(ObjectKind::Table),
        "v" => Some(ObjectKind::View),
        "m" => Some(ObjectKind::MaterializedView),
        "S" => Some(ObjectKind::Sequence),
        "function" => Some(ObjectKind::Function),
        "procedure" => Some(ObjectKind::Procedure),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_codes_map_to_object_kinds() {
        assert_eq!(object_kind("p"), Some(ObjectKind::Table));
        assert_eq!(object_kind("m"), Some(ObjectKind::MaterializedView));
        assert_eq!(object_kind("procedure"), Some(ObjectKind::Procedure));
        assert_eq!(object_kind("i"), None);
    }

    #[test]
    fn user_schemas_come_before_system_schemas() {
        let schemas = vec![
            "information_schema".to_owned(),
            "public".to_owned(),
            "pg_catalog".to_owned(),
            "app".to_owned(),
        ];
        assert_eq!(
            order_schemas(schemas.clone(), Flavor::Postgres.system_schemas()),
            vec!["app", "public", "pg_catalog", "information_schema"]
        );
        let mut cockroach = schemas;
        cockroach.push("crdb_internal".to_owned());
        assert_eq!(
            order_schemas(cockroach, Flavor::CockroachDb.system_schemas()),
            vec![
                "app",
                "public",
                "pg_catalog",
                "information_schema",
                "crdb_internal"
            ]
        );
    }
}
