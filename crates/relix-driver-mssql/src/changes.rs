/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog_rows::run_batch;
use crate::connection::sql_builder;
use crate::connector::MsSqlClient;
use crate::errors::map_error;
use crate::params::{as_sql_params, params_of};
use crate::result_sets::read_batch_results;
use crate::row_count::with_row_count;
use relix_core::{ChangeReport, ChangeSet, RowChange};
use relix_driver_api::{BuiltStatement, DriverError, ensure_single_row};

const BEGIN_SQL: &str = "BEGIN TRANSACTION";
const COMMIT_SQL: &str = "COMMIT TRANSACTION";
const ROLLBACK_SQL: &str = "IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION";

/// Applies every change of the set inside one transaction. Each statement reports its own
/// `@@ROWCOUNT` (unaffected by triggers), and updates and deletes must match exactly one
/// row, otherwise everything is rolled back.
pub async fn apply_change_set(
    client: &mut MsSqlClient,
    changes: &ChangeSet,
) -> Result<ChangeReport, DriverError> {
    let builder = sql_builder();
    let statements = changes
        .changes
        .iter()
        .map(|change| builder.change_statement(&changes.table, change))
        .collect::<Result<Vec<_>, _>>()?;
    run_batch(client, BEGIN_SQL).await?;
    let outcome = match run_changes(client, &changes.changes, &statements).await {
        Ok(report) => run_batch(client, COMMIT_SQL).await.map(|()| report),
        Err(error) => Err(error),
    };
    match outcome {
        Ok(report) => Ok(report),
        Err(error) => Err(roll_back(client, error).await),
    }
}

async fn run_changes(
    client: &mut MsSqlClient,
    changes: &[RowChange],
    statements: &[BuiltStatement],
) -> Result<ChangeReport, DriverError> {
    let mut report = ChangeReport::default();
    for (index, (change, statement)) in changes.iter().zip(statements).enumerate() {
        let affected = execute_change(client, statement).await?;
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
    client: &mut MsSqlClient,
    statement: &BuiltStatement,
) -> Result<u64, DriverError> {
    let params = params_of(&statement.params);
    let params = as_sql_params(&params);
    let stream = client
        .query(with_row_count(&statement.sql), &params)
        .await
        .map_err(map_error)?;
    let results = read_batch_results(stream, 0).await.map_err(map_error)?;
    results
        .row_count
        .ok_or_else(|| DriverError::internal("the change statement reported no row count"))
}

async fn roll_back(client: &mut MsSqlClient, error: DriverError) -> DriverError {
    match run_batch(client, ROLLBACK_SQL).await {
        Ok(()) => error,
        Err(rollback) => DriverError::connection(format!(
            "rolling back failed ({}) after: {}",
            rollback.message, error.message
        )),
    }
}
