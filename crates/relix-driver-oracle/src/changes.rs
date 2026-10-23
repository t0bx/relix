/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog_rows::{query_rows, text_at};
use crate::connection::sql_builder;
use crate::errors::map_error;
use crate::params::{as_params, bind_values};
use oracle::Connection;
use relix_core::{ChangeReport, Dialect, RowChange, TableRef};
use relix_driver_api::{BuiltStatement, DriverError, ErrorKind};

const FIRST_COLUMN_SQL: &str = "SELECT column_name FROM all_tab_columns \
     WHERE owner = :1 AND table_name = :2 ORDER BY column_id FETCH FIRST 1 ROWS ONLY";

/// Applies every change of the set inside one transaction (the data connection never
/// auto-commits). Updates and deletes must match exactly one row, otherwise everything is
/// rolled back. `table` must carry the owner as its schema.
pub fn apply_change_set(
    connection: &Connection,
    table: &TableRef,
    changes: &[RowChange],
) -> Result<ChangeReport, DriverError> {
    let statements = changes
        .iter()
        .map(|change| change_statement(connection, table, change))
        .collect::<Result<Vec<_>, _>>()?;
    let outcome = run_changes(connection, changes, &statements)
        .and_then(|report| connection.commit().map_err(map_error).map(|()| report));
    match outcome {
        Ok(report) => Ok(report),
        Err(error) => Err(match connection.rollback() {
            Ok(()) => error,
            Err(rollback) => DriverError::connection(format!(
                "rolling back failed ({rollback}) after: {}",
                error.message
            )),
        }),
    }
}

fn change_statement(
    connection: &Connection,
    table: &TableRef,
    change: &RowChange,
) -> Result<BuiltStatement, DriverError> {
    match change {
        RowChange::Insert { values } if values.is_empty() => default_insert(connection, table),
        _ => sql_builder().change_statement(table, change),
    }
}

/// Oracle has no `INSERT … DEFAULT VALUES`; setting the first column to `DEFAULT` lets
/// every column take its default.
fn default_insert(
    connection: &Connection,
    table: &TableRef,
) -> Result<BuiltStatement, DriverError> {
    let owner = table.schema.as_deref().unwrap_or_default();
    let name = table.name.as_str();
    let column = query_rows(connection, FIRST_COLUMN_SQL, &[&owner, &name])?
        .first()
        .and_then(|row| text_at(row, 0))
        .ok_or_else(|| DriverError::query(format!("table {table} has no columns")))?;
    Ok(BuiltStatement {
        sql: format!(
            "INSERT INTO {} ({}) VALUES (DEFAULT)",
            sql_builder().qualified_table(table),
            Dialect::Oracle.quote_identifier(&column)
        ),
        params: Vec::new(),
    })
}

fn run_changes(
    connection: &Connection,
    changes: &[RowChange],
    statements: &[BuiltStatement],
) -> Result<ChangeReport, DriverError> {
    let mut report = ChangeReport::default();
    for (index, (change, statement)) in changes.iter().zip(statements).enumerate() {
        let bound = bind_values(&statement.params);
        let affected = connection
            .execute(&statement.sql, &as_params(&bound))
            .and_then(|executed| executed.row_count())
            .map_err(map_error)?;
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
    Ok(report)
}

fn ensure_single_row(index: usize, operation: &str, affected: u64) -> Result<(), DriverError> {
    if affected == 1 {
        return Ok(());
    }
    Err(DriverError::new(
        ErrorKind::ConcurrentModification,
        format!(
            "change #{} ({operation}) matched {affected} rows instead of exactly one; \
             all changes were rolled back",
            index + 1
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_row_check_reports_failing_change() {
        assert!(ensure_single_row(0, "delete", 1).is_ok());
        let error = ensure_single_row(0, "update", 0).unwrap_err();
        assert_eq!(error.kind, ErrorKind::ConcurrentModification);
        assert!(error.message.contains("change #1 (update) matched 0 rows"));
    }
}
