/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::query::query_all;
use duckdb::{Connection, params};
use relix_core::{DatabaseInfo, ObjectInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;

const DATABASES_SQL: &str = "SELECT database_name, database_name = current_database() \
     FROM duckdb_databases() WHERE NOT internal ORDER BY 2 DESC, 1";

const SCHEMAS_SQL: &str =
    "SELECT schema_name FROM duckdb_schemas() WHERE database_name = ? ORDER BY schema_name";

const OBJECTS_SQL: &str = "\
    SELECT table_name, 'table', comment, 0 FROM duckdb_tables() \
     WHERE database_name = $1 AND schema_name = $2 AND NOT internal \
    UNION ALL \
    SELECT view_name, 'view', comment, 1 FROM duckdb_views() \
     WHERE database_name = $1 AND schema_name = $2 AND NOT internal \
    UNION ALL \
    SELECT DISTINCT function_name, 'function', comment, 2 FROM duckdb_functions() \
     WHERE database_name = $1 AND schema_name = $2 AND NOT internal \
       AND function_type IN ('macro', 'table_macro') \
    UNION ALL \
    SELECT sequence_name, 'sequence', comment, 3 FROM duckdb_sequences() \
     WHERE database_name = $1 AND schema_name = $2 \
    ORDER BY 4, 1";

pub(crate) fn list_databases(connection: &Connection) -> Result<Vec<DatabaseInfo>, DriverError> {
    query_all(connection, DATABASES_SQL, [], |row| {
        Ok(DatabaseInfo {
            name: row.get(0)?,
            is_default: row.get(1)?,
        })
    })
}

pub(crate) fn list_schemas(
    connection: &Connection,
    database: &str,
) -> Result<Vec<String>, DriverError> {
    query_all(connection, SCHEMAS_SQL, params![database], |row| row.get(0))
}

pub(crate) fn list_objects(
    connection: &Connection,
    database: &str,
    schema: &str,
) -> Result<Vec<ObjectInfo>, DriverError> {
    let entries = query_all(connection, OBJECTS_SQL, params![database, schema], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    Ok(entries
        .into_iter()
        .filter_map(|(name, kind, comment)| {
            Some(ObjectInfo {
                object: TableRef::new(Some(database.to_owned()), Some(schema.to_owned()), name),
                kind: object_kind(&kind)?,
                comment: comment.filter(|comment| !comment.is_empty()),
            })
        })
        .collect())
}

fn object_kind(kind: &str) -> Option<ObjectKind> {
    match kind {
        "table" => Some(ObjectKind::Table),
        "view" => Some(ObjectKind::View),
        "function" => Some(ObjectKind::Function),
        "sequence" => Some(ObjectKind::Sequence),
        _ => None,
    }
}
