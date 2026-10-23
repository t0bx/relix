/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::values::to_duckdb;
use duckdb::{Connection, params_from_iter};
use relix_core::{ChangeReport, ChangeSet, RowChange};
use relix_driver_api::{BuiltStatement, DriverError, ErrorKind, SqlBuilder};

/// Applies the whole change set inside one transaction on the data connection. Each
/// update and delete must touch exactly one row; otherwise the transaction is dropped,
/// which rolls it back. A row changed concurrently by an open editor transaction makes
/// DuckDB's optimistic concurrency control reject the commit.
pub(crate) fn apply_change_set(
    connection: &mut Connection,
    builder: SqlBuilder,
    change_set: &ChangeSet,
) -> Result<ChangeReport, DriverError> {
    let transaction = connection.transaction().map_err(map_error)?;
    let mut report = ChangeReport::default();
    for (index, change) in change_set.changes.iter().enumerate() {
        let statement = builder.change_statement(&change_set.table, change)?;
        let affected = run_statement(&transaction, &statement)?;
        match change {
            RowChange::Insert { .. } => report.inserted += affected,
            RowChange::Update { .. } => {
                ensure_single_row(index, "update", affected)?;
                report.updated += 1;
            }
            RowChange::Delete { .. } => {
                ensure_single_row(index, "delete", affected)?;
                report.deleted += 1;
            }
        }
    }
    transaction.commit().map_err(map_error)?;
    Ok(report)
}

fn run_statement(connection: &Connection, statement: &BuiltStatement) -> Result<u64, DriverError> {
    let params = statement.params.iter().map(to_duckdb);
    let affected = connection
        .prepare(&statement.sql)
        .and_then(|mut prepared| prepared.execute(params_from_iter(params)))
        .map_err(map_error)?;
    Ok(affected as u64)
}

fn ensure_single_row(index: usize, operation: &str, affected: u64) -> Result<(), DriverError> {
    if affected == 1 {
        return Ok(());
    }
    Err(DriverError::new(
        ErrorKind::ConcurrentModification,
        format!(
            "change #{} ({operation}) affected {affected} rows instead of exactly 1; \
             the row was modified or removed in the meantime, all changes were rolled back",
            index + 1
        ),
    ))
}
