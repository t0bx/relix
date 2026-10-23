/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::{map_statement_error, protocol_error};
use crate::type_names::type_name;
use crate::values::to_value;
use relix_core::{ColumnMeta, RowBatch};
use relix_driver_api::DriverError;
use scylla::client::session::Session;
use scylla::response::PagingState;
use scylla::statement::Statement;
use scylla::value::Row;
use std::ops::ControlFlow;
use std::time::Duration;

const MAX_PAGE_SIZE: u64 = 5_000;

#[derive(Debug, Clone, Copy)]
pub(crate) struct RowWindow {
    pub(crate) skip: u64,
    pub(crate) take: usize,
}

impl RowWindow {
    fn page_size(self) -> i32 {
        let wanted = self.skip.saturating_add(self.take as u64).saturating_add(1);
        i32::try_from(wanted.clamp(1, MAX_PAGE_SIZE)).unwrap_or(i32::MAX)
    }
}

/// Runs one CQL statement with driver paging and returns the rows inside the window, or
/// `None` when the statement has no result set. CQL has no OFFSET: skipped rows are
/// still read from the server page by page and discarded here, so deep pages cost as
/// much as reading every row before them. Reading stops after one row past the window,
/// which sets `has_more`; the remaining pages are never requested.
pub(crate) async fn run_paged(
    session: &Session,
    cql: &str,
    window: RowWindow,
    request_timeout: Option<Duration>,
) -> Result<Option<RowBatch>, DriverError> {
    let mut statement = Statement::new(cql.to_owned()).with_page_size(window.page_size());
    statement.set_request_timeout(request_timeout);
    let mut paging = PagingState::start();
    let mut batch: Option<RowBatch> = None;
    let mut skipped = 0u64;
    loop {
        let (result, paging_response) = session
            .query_single_page(statement.clone(), (), paging)
            .await
            .map_err(|error| map_statement_error(&error, cql))?;
        if !result.is_rows() {
            return Ok(batch);
        }
        let rows = result.into_rows_result().map_err(protocol_error)?;
        let current = batch.get_or_insert_with(|| RowBatch {
            columns: rows
                .column_specs()
                .iter()
                .map(|spec| ColumnMeta::new(spec.name(), Some(type_name(spec.typ()))))
                .collect(),
            ..RowBatch::default()
        });
        for row in rows.rows::<Row>().map_err(protocol_error)? {
            let row = row.map_err(protocol_error)?;
            if skipped < window.skip {
                skipped += 1;
                continue;
            }
            if current.rows.len() == window.take {
                current.has_more = true;
                return Ok(batch);
            }
            current
                .rows
                .push(row.columns.into_iter().map(to_value).collect());
        }
        match paging_response.into_paging_control_flow() {
            ControlFlow::Break(()) => return Ok(batch),
            ControlFlow::Continue(next) => paging = next,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_size_covers_the_window_and_one_lookahead_row() {
        assert_eq!(RowWindow { skip: 0, take: 200 }.page_size(), 201);
        assert_eq!(RowWindow { skip: 40, take: 20 }.page_size(), 61);
        assert_eq!(
            RowWindow {
                skip: 1_000_000,
                take: 20
            }
            .page_size(),
            5_000
        );
    }
}
