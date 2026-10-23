/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::client::BigQueryClient;
use crate::api::jobs::query_results;
use crate::api::models::{QueryResults, TableField, TableRow};
use crate::values::{column_meta, row_values};
use relix_core::{RowBatch, StatementOutcome};
use relix_driver_api::{DriverError, ErrorKind};

const MAX_PAGE_ROWS: u32 = 100_000;
const COMPLETED_MESSAGE: &str = "statement executed";

pub(crate) struct CollectedRows {
    pub rows: Vec<TableRow>,
    pub has_more: bool,
}

pub(crate) fn page_size(limit: usize) -> u32 {
    u32::try_from(limit)
        .unwrap_or(u32::MAX)
        .clamp(1, MAX_PAGE_ROWS)
}

/// Gathers result rows page by page: starts from the first page and hands out the next
/// page token only while fewer than `limit` rows are present. Rows beyond the limit are
/// dropped and reported as `has_more`.
pub(crate) struct PageCollector {
    rows: Vec<TableRow>,
    token: Option<String>,
    limit: usize,
}

impl PageCollector {
    pub(crate) fn new(rows: Vec<TableRow>, token: Option<String>, limit: usize) -> Self {
        Self {
            rows,
            token: token.filter(|token| !token.is_empty()),
            limit,
        }
    }

    pub(crate) fn next_token(&mut self) -> Option<String> {
        if self.rows.len() < self.limit {
            self.token.take()
        } else {
            None
        }
    }

    pub(crate) fn add_page(&mut self, rows: Vec<TableRow>, token: Option<String>) {
        self.rows.extend(rows);
        self.token = token.filter(|token| !token.is_empty());
    }

    pub(crate) fn finish(mut self) -> CollectedRows {
        let has_more = self.rows.len() > self.limit || self.token.is_some();
        self.rows.truncate(self.limit);
        CollectedRows {
            rows: self.rows,
            has_more,
        }
    }
}

pub(crate) fn row_batch(fields: &[TableField], rows: &[TableRow], has_more: bool) -> RowBatch {
    RowBatch {
        columns: fields.iter().map(column_meta).collect(),
        rows: rows.iter().map(|row| row_values(row, fields)).collect(),
        has_more,
    }
}

/// Collects up to `limit` result rows of a finished query job, paging through
/// `jobs.getQueryResults`; `totalRows` also tells whether rows were left behind.
pub(crate) async fn query_rows(
    client: &BigQueryClient,
    results: QueryResults,
    limit: usize,
) -> Result<RowBatch, DriverError> {
    let fields = results.schema.unwrap_or_default().fields;
    let total: Option<u64> = results.total_rows.and_then(|total| total.parse().ok());
    let mut pages = PageCollector::new(results.rows, results.page_token, limit);
    while let Some(token) = pages.next_token() {
        let reference = results.job_reference.as_ref().ok_or_else(|| {
            DriverError::new(
                ErrorKind::Protocol,
                "BigQuery returned a page token without a job reference",
            )
        })?;
        let page = query_results(client, reference, Some(&token), page_size(limit), None).await?;
        pages.add_page(page.rows, page.page_token);
    }
    let collected = pages.finish();
    let has_more =
        collected.has_more || total.is_some_and(|total| total > collected.rows.len() as u64);
    Ok(row_batch(&fields, &collected.rows, has_more))
}

/// DML reports its affected row count, statements without result columns (DDL, scripts
/// ending in a non-query statement) complete with a message, everything else yields rows.
pub(crate) async fn statement_outcome(
    client: &BigQueryClient,
    results: QueryResults,
    row_limit: usize,
) -> Result<StatementOutcome, DriverError> {
    if let Some(affected) = results.num_dml_affected_rows.as_deref() {
        return Ok(StatementOutcome::Affected {
            rows: affected.parse().unwrap_or_default(),
        });
    }
    let has_columns = results
        .schema
        .as_ref()
        .is_some_and(|schema| !schema.fields.is_empty());
    if !has_columns {
        return Ok(StatementOutcome::Completed {
            message: COMPLETED_MESSAGE.to_owned(),
        });
    }
    query_rows(client, results, row_limit)
        .await
        .map(StatementOutcome::Rows)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn rows(count: usize) -> Vec<TableRow> {
        vec![TableRow::default(); count]
    }

    #[test]
    fn page_tokens_are_handed_out_until_the_limit() {
        let mut pages = PageCollector::new(rows(2), Some("p2".into()), 3);
        assert_eq!(pages.next_token().as_deref(), Some("p2"));
        pages.add_page(rows(2), Some("p3".into()));
        assert_eq!(pages.next_token(), None);
        let collected = pages.finish();
        assert_eq!(collected.rows.len(), 3);
        assert!(collected.has_more);
    }

    #[test]
    fn last_page_without_token_has_no_more_rows() {
        let mut pages = PageCollector::new(rows(1), Some("p2".into()), 10);
        assert!(pages.next_token().is_some());
        pages.add_page(rows(1), None);
        assert_eq!(pages.next_token(), None);
        let collected = pages.finish();
        assert_eq!(collected.rows.len(), 2);
        assert!(!collected.has_more);
        assert_eq!(page_size(0), 1);
        assert_eq!(page_size(usize::MAX), MAX_PAGE_ROWS);
    }
}
