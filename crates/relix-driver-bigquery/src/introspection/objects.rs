/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::catalog;
use crate::connection::BigQueryConnection;
use relix_core::{ObjectInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;

/// Lists the tables, views and routines of a dataset. Listing routines needs its own
/// permission (`bigquery.routines.list`) that table readers often lack, and not every
/// API implementation offers it, so tables stay browsable when that listing fails.
pub(crate) async fn list(
    connection: &BigQueryConnection,
    project: &str,
    dataset: &str,
) -> Result<Vec<ObjectInfo>, DriverError> {
    let client = connection.client();
    let (tables, routines) = tokio::join!(
        catalog::tables(client, project, dataset),
        catalog::routines(client, project, dataset)
    );
    let object = |name: String, kind: ObjectKind| ObjectInfo {
        object: TableRef::new(Some(project.to_owned()), Some(dataset.to_owned()), name),
        kind,
        comment: None,
    };
    let mut objects: Vec<ObjectInfo> = tables?
        .into_iter()
        .map(|entry| {
            let kind = table_kind(entry.table_type.as_deref());
            object(entry.table_reference.table_id, kind)
        })
        .collect();
    if let Ok(routines) = routines {
        objects.extend(routines.into_iter().map(|entry| {
            let kind = routine_kind(entry.routine_type.as_deref());
            object(entry.routine_reference.routine_id, kind)
        }));
    }
    Ok(objects)
}

pub(crate) fn table_kind(table_type: Option<&str>) -> ObjectKind {
    match table_type.map(str::to_ascii_uppercase).as_deref() {
        Some("VIEW") => ObjectKind::View,
        Some("MATERIALIZED_VIEW") => ObjectKind::MaterializedView,
        _ => ObjectKind::Table,
    }
}

fn routine_kind(routine_type: Option<&str>) -> ObjectKind {
    match routine_type.map(str::to_ascii_uppercase).as_deref() {
        Some("PROCEDURE") => ObjectKind::Procedure,
        _ => ObjectKind::Function,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_types_map_to_explorer_kinds() {
        assert_eq!(table_kind(Some("TABLE")), ObjectKind::Table);
        assert_eq!(table_kind(Some("EXTERNAL")), ObjectKind::Table);
        assert_eq!(table_kind(Some("VIEW")), ObjectKind::View);
        assert_eq!(
            table_kind(Some("MATERIALIZED_VIEW")),
            ObjectKind::MaterializedView
        );
        assert_eq!(routine_kind(Some("PROCEDURE")), ObjectKind::Procedure);
        assert_eq!(
            routine_kind(Some("TABLE_VALUED_FUNCTION")),
            ObjectKind::Function
        );
    }
}
