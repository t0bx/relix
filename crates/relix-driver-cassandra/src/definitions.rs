/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::cql_rows::select_rows;
use crate::table_names::qualified_name;
use relix_core::ObjectKind;
use scylla::client::session::Session;

type DescribeRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Returns the server's `DESCRIBE` output (Cassandra 4.0+, ScyllaDB 5.2+), which for a
/// table also contains its indexes and views. Older servers do not support server-side
/// `DESCRIBE`, in which case no definition is shown.
pub(crate) async fn describe(
    session: &Session,
    keyspace: &str,
    name: &str,
    kind: ObjectKind,
) -> Option<String> {
    let object = match kind {
        ObjectKind::MaterializedView => "MATERIALIZED VIEW",
        _ => "TABLE",
    };
    let cql = format!("DESCRIBE {object} {}", qualified_name(keyspace, name));
    let rows: Vec<DescribeRow> = select_rows(session, &cql, ()).await.ok()?;
    let statements: Vec<String> = rows
        .into_iter()
        .filter_map(|(_, _, _, statement)| statement)
        .collect();
    (!statements.is_empty()).then(|| statements.join("\n\n"))
}
