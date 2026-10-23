/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::SnowflakeConnection;
use crate::introspection::show_table::ShowTable;
use crate::introspection::{identifier, show};
use relix_core::{ObjectInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;

/// Lists tables and views (`SHOW TERSE OBJECTS`), user functions, procedures and
/// sequences of one schema. `SHOW` commands are metadata operations that need no running
/// warehouse; the four commands run concurrently.
pub(crate) async fn list(
    connection: &SnowflakeConnection,
    database: &str,
    schema: &str,
) -> Result<Vec<ObjectInfo>, DriverError> {
    let scope = format!("{}.{}", identifier(database), identifier(schema));
    let [relations_sql, functions_sql, procedures_sql, sequences_sql] =
        ["TERSE OBJECTS", "USER FUNCTIONS", "PROCEDURES", "SEQUENCES"]
            .map(|listing| format!("SHOW {listing} IN SCHEMA {scope}"));
    let (relations, functions, procedures, sequences) = tokio::try_join!(
        show(connection, &relations_sql),
        show(connection, &functions_sql),
        show(connection, &procedures_sql),
        show(connection, &sequences_sql),
    )?;
    let mut objects = Vec::new();
    let mut push = |name: &str, kind: ObjectKind| {
        let duplicate = objects
            .iter()
            .any(|existing: &ObjectInfo| existing.kind == kind && existing.object.name == name);
        if !duplicate {
            objects.push(ObjectInfo {
                object: TableRef::new(Some(database.to_owned()), Some(schema.to_owned()), name),
                kind,
                comment: None,
            });
        }
    };
    for row in relations.rows() {
        if let Some(name) = row.get("name") {
            push(name, relation_kind(row.get("kind").unwrap_or_default()));
        }
    }
    for (routines, kind) in [
        (&functions, ObjectKind::Function),
        (&procedures, ObjectKind::Procedure),
    ] {
        for name in user_defined_names(routines) {
            push(name, kind);
        }
    }
    for row in sequences.rows() {
        if let Some(name) = row.get("name") {
            push(name, ObjectKind::Sequence);
        }
    }
    Ok(objects)
}

pub(crate) fn relation_kind(kind: &str) -> ObjectKind {
    let kind = kind.to_ascii_uppercase();
    if kind.contains("MATERIALIZED") {
        ObjectKind::MaterializedView
    } else if kind.contains("VIEW") {
        ObjectKind::View
    } else {
        ObjectKind::Table
    }
}

fn user_defined_names(routines: &ShowTable) -> impl Iterator<Item = &str> {
    routines
        .rows()
        .filter(|row| !row.flag("is_builtin"))
        .filter_map(|row| row.get("name"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_kinds_map_to_explorer_kinds() {
        assert_eq!(relation_kind("TABLE"), ObjectKind::Table);
        assert_eq!(relation_kind("VIEW"), ObjectKind::View);
        assert_eq!(
            relation_kind("MATERIALIZED_VIEW"),
            ObjectKind::MaterializedView
        );
        assert_eq!(relation_kind("DYNAMIC_TABLE"), ObjectKind::Table);
    }
}
