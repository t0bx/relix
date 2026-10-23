/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog_rows::{query_rows, text_at};
use oracle::Connection;
use relix_core::{ObjectInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;

pub const PACKAGE_COMMENT: &str = "package";

const SCHEMAS_SQL: &str = "SELECT username FROM all_users \
     WHERE oracle_maintained = 'N' OR username = SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') \
     ORDER BY username";

const OBJECTS_SQL: &str = "SELECT o.object_name, o.object_type, \
       COALESCE(tc.comments, mc.comments) \
     FROM all_objects o \
     LEFT JOIN all_tab_comments tc ON tc.owner = o.owner AND tc.table_name = o.object_name \
       AND o.object_type IN ('TABLE', 'VIEW') \
     LEFT JOIN all_mview_comments mc ON mc.owner = o.owner AND mc.mview_name = o.object_name \
       AND o.object_type = 'MATERIALIZED VIEW' \
     WHERE o.owner = :1 \
       AND o.object_type IN ('TABLE', 'VIEW', 'MATERIALIZED VIEW', 'SEQUENCE', 'PROCEDURE', \
         'FUNCTION', 'PACKAGE', 'TRIGGER') \
       AND o.generated = 'N' AND o.secondary = 'N' \
       AND o.object_name NOT LIKE 'BIN$%' AND o.object_name NOT LIKE 'ISEQ$$%' \
       AND NOT (o.object_type = 'TABLE' AND EXISTS (SELECT 1 FROM all_mviews m \
         WHERE m.owner = o.owner AND m.mview_name = o.object_name)) \
     ORDER BY o.object_name, o.object_type";

pub fn list_schemas(connection: &Connection) -> Result<Vec<String>, DriverError> {
    let rows = query_rows(connection, SCHEMAS_SQL, &[])?;
    Ok(rows.iter().filter_map(|row| text_at(row, 0)).collect())
}

pub fn list_objects(
    connection: &Connection,
    database: &str,
    schema: &str,
) -> Result<Vec<ObjectInfo>, DriverError> {
    let rows = query_rows(connection, OBJECTS_SQL, &[&schema])?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let object_type = text_at(row, 1)?;
            let comment = if object_type == "PACKAGE" {
                Some(PACKAGE_COMMENT.to_owned())
            } else {
                text_at(row, 2).filter(|comment| !comment.is_empty())
            };
            Some(ObjectInfo {
                object: TableRef::new(
                    Some(database.to_owned()),
                    Some(schema.to_owned()),
                    text_at(row, 0)?,
                ),
                kind: object_kind(&object_type)?,
                comment,
            })
        })
        .collect())
}

pub fn object_kind(object_type: &str) -> Option<ObjectKind> {
    match object_type {
        "TABLE" => Some(ObjectKind::Table),
        "VIEW" => Some(ObjectKind::View),
        "MATERIALIZED VIEW" => Some(ObjectKind::MaterializedView),
        "SEQUENCE" => Some(ObjectKind::Sequence),
        "PROCEDURE" | "PACKAGE" => Some(ObjectKind::Procedure),
        "FUNCTION" => Some(ObjectKind::Function),
        "TRIGGER" => Some(ObjectKind::Trigger),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_types_map_to_kinds() {
        assert_eq!(object_kind("TABLE"), Some(ObjectKind::Table));
        assert_eq!(
            object_kind("MATERIALIZED VIEW"),
            Some(ObjectKind::MaterializedView)
        );
        assert_eq!(object_kind("PACKAGE"), Some(ObjectKind::Procedure));
        assert_eq!(object_kind("PACKAGE BODY"), None);
        assert_eq!(object_kind("INDEX"), None);
    }
}
