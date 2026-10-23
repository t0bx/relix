/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog_rows::{catalog_query, non_empty_text_at, text_at};
use mysql_async::Conn;
use relix_core::{ObjectInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;

const DATABASES_SQL: &str =
    "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME";

const RELATIONS_SQL: &str = "SELECT TABLE_NAME, TABLE_TYPE, TABLE_COMMENT \
     FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME";

const ROUTINES_SQL: &str = "SELECT ROUTINE_NAME, ROUTINE_TYPE, ROUTINE_COMMENT \
     FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_NAME";

pub async fn list_databases(conn: &mut Conn) -> Result<Vec<String>, DriverError> {
    let rows = catalog_query(conn, DATABASES_SQL, &[]).await?;
    Ok(rows.iter().filter_map(|row| text_at(row, 0)).collect())
}

pub async fn list_objects(conn: &mut Conn, database: &str) -> Result<Vec<ObjectInfo>, DriverError> {
    let relations = catalog_query(conn, RELATIONS_SQL, &[database]).await?;
    let routines = catalog_query(conn, ROUTINES_SQL, &[database]).await?;
    let relation_objects = relations.iter().filter_map(|row| {
        let kind = relation_kind(&text_at(row, 1)?)?;
        let comment = non_empty_text_at(row, 2).filter(|comment| comment != "VIEW");
        Some(object(database, text_at(row, 0)?, kind, comment))
    });
    let routine_objects = routines.iter().filter_map(|row| {
        let kind = routine_kind(&text_at(row, 1)?)?;
        Some(object(
            database,
            text_at(row, 0)?,
            kind,
            non_empty_text_at(row, 2),
        ))
    });
    Ok(relation_objects.chain(routine_objects).collect())
}

pub fn relation_kind(table_type: &str) -> Option<ObjectKind> {
    match table_type {
        "BASE TABLE" | "SYSTEM VERSIONED" | "TEMPORARY" => Some(ObjectKind::Table),
        "VIEW" | "SYSTEM VIEW" => Some(ObjectKind::View),
        "SEQUENCE" => Some(ObjectKind::Sequence),
        _ => None,
    }
}

fn routine_kind(routine_type: &str) -> Option<ObjectKind> {
    match routine_type {
        "FUNCTION" => Some(ObjectKind::Function),
        "PROCEDURE" => Some(ObjectKind::Procedure),
        _ => None,
    }
}

fn object(database: &str, name: String, kind: ObjectKind, comment: Option<String>) -> ObjectInfo {
    ObjectInfo {
        object: TableRef::new(Some(database.to_owned()), None, name),
        kind,
        comment,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_types_map_to_object_kinds() {
        assert_eq!(relation_kind("BASE TABLE"), Some(ObjectKind::Table));
        assert_eq!(relation_kind("SYSTEM VERSIONED"), Some(ObjectKind::Table));
        assert_eq!(relation_kind("SYSTEM VIEW"), Some(ObjectKind::View));
        assert_eq!(relation_kind("SEQUENCE"), Some(ObjectKind::Sequence));
        assert_eq!(relation_kind("UNKNOWN"), None);
        assert_eq!(routine_kind("PROCEDURE"), Some(ObjectKind::Procedure));
        assert_eq!(routine_kind("PACKAGE"), None);
    }
}
