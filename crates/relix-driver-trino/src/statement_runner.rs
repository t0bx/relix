/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::TrinoClient;
use crate::errors::map_query_error;
use crate::protocol::ResultColumn;
use crate::session_state::SessionState;
use crate::values::from_json;
use relix_core::{ColumnMeta, RowBatch, Value};
use relix_driver_api::{DriverError, ErrorKind};
use std::sync::{Mutex, PoisonError};

pub(crate) struct QueryOutcome {
    pub(crate) batch: Option<RowBatch>,
    pub(crate) update_type: Option<String>,
    pub(crate) update_count: Option<u64>,
}

/// Remembers the `nextUri` of the statement in flight so another task can cancel it.
#[derive(Default)]
pub(crate) struct RunningQuery {
    next_uri: Mutex<Option<String>>,
}

impl RunningQuery {
    pub(crate) fn current(&self) -> Option<String> {
        self.next_uri
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set(&self, next_uri: Option<String>) {
        *self.next_uri.lock().unwrap_or_else(PoisonError::into_inner) = next_uri;
    }
}

/// Runs one statement through the client protocol: submits it, follows `nextUri`
/// until the query finishes and applies session changes announced on the way. Rows are
/// collected up to `limit`; once one more row arrives the rest of the result is
/// abandoned by deleting the `nextUri`, which stops the query on the server.
pub(crate) async fn run_statement(
    client: &TrinoClient,
    sql: &str,
    session: &mut SessionState,
    limit: usize,
    running: Option<&RunningQuery>,
) -> Result<QueryOutcome, DriverError> {
    let outcome = follow_pages(client, sql, session, limit, running).await;
    if let Some(running) = running {
        running.set(None);
    }
    outcome
}

async fn follow_pages(
    client: &TrinoClient,
    sql: &str,
    session: &mut SessionState,
    limit: usize,
    running: Option<&RunningQuery>,
) -> Result<QueryOutcome, DriverError> {
    let mut response = client.submit(sql, session).await?;
    let mut columns: Option<Vec<ResultColumn>> = None;
    let mut rows: Vec<Vec<Value>> = Vec::new();
    let mut has_more = false;
    let mut update_type = None;
    let mut update_count = None;
    loop {
        session.update(response.headers());
        let page = client.page(response).await?;
        if let Some(error) = page.error {
            return Err(map_query_error(error, sql));
        }
        if columns.is_none() {
            columns = page.columns;
        }
        update_type = page.update_type.or(update_type);
        update_count = page.update_count.or(update_count);
        if let Some(data) = page.data {
            let types = columns.as_deref().ok_or_else(|| {
                DriverError::new(ErrorKind::Protocol, "Trino sent rows before columns")
            })?;
            for cells in data {
                if rows.len() == limit {
                    has_more = true;
                    break;
                }
                rows.push(
                    cells
                        .iter()
                        .zip(types)
                        .map(|(cell, column)| from_json(cell, &column.type_name))
                        .collect(),
                );
            }
        }
        let Some(next_uri) = page.next_uri else {
            break;
        };
        if has_more {
            let _ = client.abandon(&next_uri).await;
            break;
        }
        if let Some(running) = running {
            running.set(Some(next_uri.clone()));
        }
        response = client.next_page(&next_uri).await?;
    }
    Ok(QueryOutcome {
        batch: columns.map(|columns| RowBatch {
            columns: columns
                .into_iter()
                .map(|column| ColumnMeta::new(column.name, Some(column.type_name)))
                .collect(),
            rows,
            has_more,
        }),
        update_type,
        update_count,
    })
}
