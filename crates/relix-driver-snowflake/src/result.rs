/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::client::SqlApiClient;
use crate::api::models::{ResultSet, RowType};
use crate::values::{cell_text, column_meta, convert};
use relix_core::{RowBatch, StatementOutcome};
use relix_driver_api::{DriverError, ErrorKind};
use relix_query::StatementKind;

const COMPLETED_MESSAGE: &str = "statement executed";

pub(crate) struct RawRows {
    pub columns: Vec<RowType>,
    pub rows: Vec<Vec<Option<String>>>,
    pub has_more: bool,
}

impl RawRows {
    pub(crate) fn into_batch(self) -> RowBatch {
        let rows = self
            .rows
            .iter()
            .map(|row| {
                self.columns
                    .iter()
                    .enumerate()
                    .map(|(index, column)| {
                        convert(row.get(index).cloned().flatten().as_deref(), column)
                    })
                    .collect()
            })
            .collect();
        RowBatch {
            columns: self.columns.iter().map(column_meta).collect(),
            rows,
            has_more: self.has_more,
        }
    }
}

/// Collects up to `limit` rows. The first partition comes with the statement result;
/// further partitions are only requested while the limit is not reached, and `has_more`
/// is derived from the total row count Snowflake reports.
pub(crate) async fn collect_rows(
    client: &SqlApiClient,
    first: ResultSet,
    limit: usize,
) -> Result<RawRows, DriverError> {
    let meta = first.result_set_meta_data.unwrap_or_default();
    let mut rows = raw_rows(&first.data);
    let mut partition = 1;
    while rows.len() < limit && partition < meta.partition_info.len() {
        let handle = first.statement_handle.as_deref().ok_or_else(|| {
            DriverError::new(
                ErrorKind::Protocol,
                "Snowflake returned result partitions without a statement handle",
            )
        })?;
        rows.extend(raw_rows(&client.partition(handle, partition).await?.data));
        partition += 1;
    }
    let total = meta.num_rows.max(rows.len() as u64);
    rows.truncate(limit);
    Ok(RawRows {
        columns: meta.row_type,
        has_more: total > rows.len() as u64,
        rows,
    })
}

/// Turns a finished statement into its outcome: DML statistics become an affected-row
/// count, the single `status` column of DDL and session commands a completion message,
/// and everything else a row batch.
pub(crate) async fn statement_outcome(
    client: &SqlApiClient,
    first: ResultSet,
    kind: StatementKind,
    row_limit: usize,
) -> Result<StatementOutcome, DriverError> {
    if let Some(stats) = first.stats {
        return Ok(StatementOutcome::Affected {
            rows: stats.affected_rows(),
        });
    }
    let Some(meta) = first.result_set_meta_data.as_ref() else {
        return Ok(StatementOutcome::Completed {
            message: first
                .message
                .unwrap_or_else(|| COMPLETED_MESSAGE.to_owned()),
        });
    };
    let is_status_reply =
        meta.row_type.len() == 1 && meta.row_type[0].name.eq_ignore_ascii_case("status");
    if kind != StatementKind::Query && is_status_reply {
        let message = first
            .data
            .first()
            .and_then(|row| row.first())
            .and_then(cell_text)
            .unwrap_or_else(|| COMPLETED_MESSAGE.to_owned());
        return Ok(StatementOutcome::Completed { message });
    }
    let rows = collect_rows(client, first, row_limit).await?;
    Ok(StatementOutcome::Rows(rows.into_batch()))
}

fn raw_rows(data: &[Vec<serde_json::Value>]) -> Vec<Vec<Option<String>>> {
    data.iter()
        .map(|row| row.iter().map(cell_text).collect())
        .collect()
}
