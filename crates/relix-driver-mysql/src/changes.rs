/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::sql_builder;
use crate::errors::map_error;
use crate::values::{statement_params, to_mysql};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Transaction, TxOpts};
use relix_core::{ChangeReport, ChangeSet, RowChange};
use relix_driver_api::{BuiltStatement, DriverError, ensure_single_row};

/// Applies every change of the set inside one transaction. Updates and deletes must match
/// exactly one row (the connection reports matched rows), otherwise everything is rolled back.
pub async fn apply_change_set(
    conn: &mut Conn,
    changes: &ChangeSet,
) -> Result<ChangeReport, DriverError> {
    let builder = sql_builder();
    let statements = changes
        .changes
        .iter()
        .map(|change| builder.change_statement(&changes.table, change))
        .collect::<Result<Vec<_>, _>>()?;
    let mut transaction = conn
        .start_transaction(TxOpts::default())
        .await
        .map_err(map_error)?;
    match run_changes(&mut transaction, &changes.changes, &statements).await {
        Ok(report) => {
            transaction.commit().await.map_err(map_error)?;
            Ok(report)
        }
        Err(error) => {
            let _ = transaction.rollback().await;
            Err(error)
        }
    }
}

async fn run_changes(
    transaction: &mut Transaction<'_>,
    changes: &[RowChange],
    statements: &[BuiltStatement],
) -> Result<ChangeReport, DriverError> {
    let mut report = ChangeReport::default();
    for (index, (change, statement)) in changes.iter().zip(statements).enumerate() {
        let affected = execute_change(transaction, statement).await?;
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

async fn execute_change(
    transaction: &mut Transaction<'_>,
    statement: &BuiltStatement,
) -> Result<u64, DriverError> {
    let params = statement_params(statement.params.iter().map(to_mysql).collect());
    let result = transaction
        .exec_iter(statement.sql.as_str(), params)
        .await
        .map_err(map_error)?;
    let affected = result.affected_rows();
    result.drop_result().await.map_err(map_error)?;
    Ok(affected)
}
