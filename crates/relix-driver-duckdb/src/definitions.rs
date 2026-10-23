/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::query::{query_all, text_list};
use duckdb::{Connection, params};
use relix_core::{Dialect, ObjectInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;

pub(crate) const DEFAULT_SCHEMA: &str = "main";

const RELATION_SQL: &str = "\
    SELECT 'table', sql FROM duckdb_tables() \
     WHERE database_name = $1 AND schema_name = $2 AND table_name = $3 \
    UNION ALL \
    SELECT 'view', sql FROM duckdb_views() \
     WHERE database_name = $1 AND schema_name = $2 AND view_name = $3";

const MACROS_SQL: &str = "SELECT function_type, parameters, macro_definition \
     FROM duckdb_functions() WHERE database_name = ? AND schema_name = ? AND function_name = ? \
       AND function_type IN ('macro', 'table_macro') AND NOT internal";

const SEQUENCE_SQL: &str = "SELECT sql FROM duckdb_sequences() \
     WHERE database_name = ? AND schema_name = ? AND sequence_name = ?";

pub(crate) fn database_of(
    connection: &Connection,
    table: &TableRef,
) -> Result<String, DriverError> {
    match table.database.as_deref().filter(|name| !name.is_empty()) {
        Some(database) => Ok(database.to_owned()),
        None => connection
            .query_row("SELECT current_database()", [], |row| row.get(0))
            .map_err(map_error),
    }
}

pub(crate) fn relation_definition(
    connection: &Connection,
    database: &str,
    schema: &str,
    name: &str,
) -> Result<Option<(ObjectKind, Option<String>)>, DriverError> {
    let rows = query_all(
        connection,
        RELATION_SQL,
        params![database, schema, name],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )?;
    Ok(rows.into_iter().next().map(|(kind, sql)| {
        let kind = if kind == "view" {
            ObjectKind::View
        } else {
            ObjectKind::Table
        };
        (kind, sql)
    }))
}

pub(crate) fn object_definition(
    connection: &Connection,
    object: &ObjectInfo,
) -> Result<Option<String>, DriverError> {
    let database = database_of(connection, &object.object)?;
    let schema = object.object.schema.as_deref().unwrap_or(DEFAULT_SCHEMA);
    let name = object.object.name.as_str();
    match object.kind {
        ObjectKind::Table | ObjectKind::View => {
            Ok(relation_definition(connection, &database, schema, name)?.and_then(|(_, sql)| sql))
        }
        ObjectKind::Function => macro_definition(connection, &database, schema, name),
        ObjectKind::Sequence => Ok(query_all(
            connection,
            SEQUENCE_SQL,
            params![database, schema, name],
            |row| row.get::<_, Option<String>>(0),
        )?
        .into_iter()
        .flatten()
        .next()),
        _ => Ok(None),
    }
}

/// DuckDB keeps no DDL text for macros, so the statement is rebuilt from the catalog:
/// one `CREATE MACRO` per overload, with its parameters and body.
fn macro_definition(
    connection: &Connection,
    database: &str,
    schema: &str,
    name: &str,
) -> Result<Option<String>, DriverError> {
    let overloads = query_all(
        connection,
        MACROS_SQL,
        params![database, schema, name],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                text_list(row.get_ref(1)?),
                row.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    let qualified = format!(
        "{}.{}",
        Dialect::DuckDb.quote_identifier(schema),
        Dialect::DuckDb.quote_identifier(name)
    );
    let statements: Vec<String> = overloads
        .into_iter()
        .map(|(kind, parameters, body)| {
            let table = if kind == "table_macro" { "TABLE " } else { "" };
            format!(
                "CREATE MACRO {qualified}({}) AS {table}{};",
                parameters.join(", "),
                body.unwrap_or_default()
            )
        })
        .collect();
    Ok((!statements.is_empty()).then(|| statements.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(name: &str, kind: ObjectKind) -> ObjectInfo {
        ObjectInfo {
            object: TableRef::new(None, Some(DEFAULT_SCHEMA.into()), name),
            kind,
            comment: None,
        }
    }

    #[test]
    fn definitions_exist_for_every_listed_kind() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE VIEW v AS SELECT 1 AS one; CREATE SEQUENCE s; \
                 CREATE MACRO add_one(a) AS a + 1; CREATE MACRO rows_of(x) AS TABLE SELECT x;",
            )
            .unwrap();
        let view = object_definition(&connection, &object("v", ObjectKind::View)).unwrap();
        assert_eq!(view.as_deref(), Some("CREATE VIEW v AS SELECT 1 AS one;"));
        let sequence = object_definition(&connection, &object("s", ObjectKind::Sequence));
        assert!(sequence.unwrap().unwrap().starts_with("CREATE SEQUENCE s"));
        let scalar = object_definition(&connection, &object("add_one", ObjectKind::Function));
        assert_eq!(
            scalar.unwrap().as_deref(),
            Some("CREATE MACRO \"main\".\"add_one\"(a) AS (a + 1);")
        );
        let table = object_definition(&connection, &object("rows_of", ObjectKind::Function));
        assert!(table.unwrap().unwrap().contains("AS TABLE SELECT x"));
    }
}
