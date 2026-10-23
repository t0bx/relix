/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::query::query_all;
use relix_core::{DatabaseInfo, Dialect, ObjectInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;
use rusqlite::{Connection, params};

pub(crate) const MAIN_DATABASE: &str = "main";
const TEMP_DATABASE: &str = "temp";

pub(crate) struct MasterEntry {
    pub(crate) kind: ObjectKind,
    pub(crate) sql: Option<String>,
}

pub(crate) fn list_databases(connection: &Connection) -> Result<Vec<DatabaseInfo>, DriverError> {
    let names = query_all(
        connection,
        "SELECT name FROM pragma_database_list ORDER BY seq",
        [],
        |row| row.get::<_, String>(0),
    )?;
    Ok(names
        .into_iter()
        .filter(|name| name != TEMP_DATABASE)
        .map(|name| DatabaseInfo {
            is_default: name == MAIN_DATABASE,
            name,
        })
        .collect())
}

pub(crate) fn list_objects(
    connection: &Connection,
    database: &str,
) -> Result<Vec<ObjectInfo>, DriverError> {
    let sql = format!(
        "SELECT type, name FROM {}.sqlite_master \
         WHERE type IN ('table', 'view', 'trigger') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
         ORDER BY CASE type WHEN 'table' THEN 0 WHEN 'view' THEN 1 ELSE 2 END, name",
        Dialect::Sqlite.quote_identifier(database)
    );
    let entries = query_all(connection, &sql, [], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    Ok(entries
        .into_iter()
        .filter_map(|(kind, name)| {
            Some(ObjectInfo {
                object: TableRef::new(Some(database.to_owned()), None, name),
                kind: object_kind(&kind)?,
                comment: None,
            })
        })
        .collect())
}

pub(crate) fn master_entry(
    connection: &Connection,
    database: &str,
    name: &str,
    kinds: &[ObjectKind],
) -> Result<Option<MasterEntry>, DriverError> {
    let sql = format!(
        "SELECT type, sql FROM {}.sqlite_master WHERE name = ?1",
        Dialect::Sqlite.quote_identifier(database)
    );
    let mut statement = connection.prepare(&sql).map_err(map_error)?;
    let mut rows = statement.query(params![name]).map_err(map_error)?;
    while let Some(row) = rows.next().map_err(map_error)? {
        let kind = object_kind(&row.get::<_, String>(0).map_err(map_error)?);
        if let Some(kind) = kind.filter(|kind| kinds.contains(kind)) {
            return Ok(Some(MasterEntry {
                kind,
                sql: row.get(1).map_err(map_error)?,
            }));
        }
    }
    Ok(None)
}

pub(crate) fn database_of(table: &TableRef) -> &str {
    table.database.as_deref().unwrap_or(MAIN_DATABASE)
}

fn object_kind(kind: &str) -> Option<ObjectKind> {
    match kind {
        "table" => Some(ObjectKind::Table),
        "view" => Some(ObjectKind::View),
        "trigger" => Some(ObjectKind::Trigger),
        _ => None,
    }
}
