/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::cql_rows::select_rows;
use relix_core::{DatabaseInfo, ObjectInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;
use scylla::client::session::Session;
use std::collections::HashSet;

const KEYSPACES_CQL: &str = "SELECT keyspace_name FROM system_schema.keyspaces";
const TABLES_CQL: &str =
    "SELECT table_name, comment FROM system_schema.tables WHERE keyspace_name = ?";
const VIEWS_CQL: &str =
    "SELECT view_name, base_table_name FROM system_schema.views WHERE keyspace_name = ?";
const INDEX_NAMES_CQL: &str =
    "SELECT index_name FROM system_schema.indexes WHERE keyspace_name = ?";
const SCYLLA_INDEX_VIEW_SUFFIX: &str = "_index";

pub(crate) async fn keyspaces(
    session: &Session,
    default_keyspace: Option<&str>,
) -> Result<Vec<DatabaseInfo>, DriverError> {
    let rows: Vec<(String,)> = select_rows(session, KEYSPACES_CQL, ()).await?;
    let mut names: Vec<String> = rows.into_iter().map(|(name,)| name).collect();
    names.sort_by(|left, right| {
        is_system_keyspace(left)
            .cmp(&is_system_keyspace(right))
            .then_with(|| left.cmp(right))
    });
    Ok(names
        .into_iter()
        .map(|name| DatabaseInfo {
            is_default: default_keyspace == Some(name.as_str()),
            name,
        })
        .collect())
}

/// Lists tables and materialized views. ScyllaDB implements secondary indexes as
/// materialized views named `<index>_index`; those are internal and are shown as
/// indexes of their table instead of as views.
pub(crate) async fn objects(
    session: &Session,
    keyspace: &str,
) -> Result<Vec<ObjectInfo>, DriverError> {
    let tables: Vec<(String, Option<String>)> =
        select_rows(session, TABLES_CQL, (keyspace,)).await?;
    let views: Vec<(String, Option<String>)> = select_rows(session, VIEWS_CQL, (keyspace,)).await?;
    let index_views: HashSet<String> =
        select_rows::<(String,)>(session, INDEX_NAMES_CQL, (keyspace,))
            .await?
            .into_iter()
            .map(|(index,)| format!("{index}{SCYLLA_INDEX_VIEW_SUFFIX}"))
            .collect();
    let object = |name: String, kind: ObjectKind, comment: Option<String>| ObjectInfo {
        object: TableRef::new(Some(keyspace.to_owned()), None, name),
        kind,
        comment: comment.filter(|comment| !comment.trim().is_empty()),
    };
    let mut objects: Vec<ObjectInfo> = tables
        .into_iter()
        .map(|(name, comment)| object(name, ObjectKind::Table, comment))
        .collect();
    objects.extend(
        views
            .into_iter()
            .filter(|(name, _)| !index_views.contains(name))
            .map(|(name, base)| {
                let comment = base.map(|base| format!("view of {base}"));
                object(name, ObjectKind::MaterializedView, comment)
            }),
    );
    Ok(objects)
}

fn is_system_keyspace(name: &str) -> bool {
    name.starts_with("system")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_keyspaces_are_recognised() {
        for name in [
            "system",
            "system_schema",
            "system_auth",
            "system_distributed",
        ] {
            assert!(is_system_keyspace(name));
        }
        assert!(!is_system_keyspace("shop"));
    }
}
