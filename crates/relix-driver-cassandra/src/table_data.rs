/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::change_statements::ChangeTarget;
use crate::connection::CassandraConnection;
use crate::cql_rows::select_rows;
use crate::page_statement::{count_statement, page_statement};
use crate::paged_query::{RowWindow, run_paged};
use crate::schema_columns::load_columns;
use crate::table_details::object_kind;
use crate::table_names::keyspace_of;
use async_trait::async_trait;
use relix_core::{ChangeReport, ChangeSet, ObjectKind, RowBatch, RowChange, TableQuery, TableRef};
use relix_driver_api::{DriverError, ErrorKind, TableDataAccess};
use scylla::client::session::Session;
use scylla::value::{CqlValue, Row};

#[async_trait]
impl TableDataAccess for CassandraConnection {
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        let live = self.live().await?;
        let keyspace = keyspace_of(table, self.default_keyspace.as_deref())?;
        let cql = page_statement(keyspace, &table.name, query);
        let window = RowWindow {
            skip: query.offset,
            take: query.limit as usize,
        };
        run_paged(&live.session, &cql, window, self.query_timeout)
            .await?
            .ok_or_else(|| DriverError::internal("page query returned no result set"))
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        let live = self.live().await?;
        let keyspace = keyspace_of(table, self.default_keyspace.as_deref())?;
        let cql = count_statement(keyspace, &table.name, filter);
        let rows: Vec<(i64,)> = select_rows(&live.session, &cql, ()).await?;
        let count = rows.first().map(|(count,)| *count).unwrap_or_default();
        Ok(u64::try_from(count).unwrap_or_default())
    }

    /// Applies the change set statement by statement. CQL has no multi-statement
    /// transactions (a logged batch is atomic but cannot contain conditional statements
    /// for different partitions), so every change is its own lightweight transaction and
    /// earlier changes stay saved when a later one fails; the error says how many.
    async fn apply_changes(&self, changes: &ChangeSet) -> Result<ChangeReport, DriverError> {
        if changes.is_empty() {
            return Ok(ChangeReport::default());
        }
        let live = self.live().await?;
        let keyspace = keyspace_of(&changes.table, self.default_keyspace.as_deref())?;
        let name = changes.table.name.as_str();
        if object_kind(&live.session, keyspace, name).await? == ObjectKind::MaterializedView {
            return Err(DriverError::unsupported(
                "materialized views are read-only; edit the base table instead",
            ));
        }
        let columns = load_columns(&live.session, keyspace, name).await?;
        let target = ChangeTarget {
            keyspace,
            table: name,
            columns: &columns,
        };
        let statements = changes
            .changes
            .iter()
            .map(|change| target.statement(change))
            .collect::<Result<Vec<_>, _>>()?;
        let mut report = ChangeReport::default();
        for (index, (change, cql)) in changes.changes.iter().zip(&statements).enumerate() {
            let applied = execute_conditional(&live.session, cql)
                .await
                .map_err(|error| with_saved_note(error, index))?;
            if !applied {
                return Err(not_applied(index, change));
            }
            match change {
                RowChange::Insert { .. } => report.inserted += 1,
                RowChange::Update { .. } => report.updated += 1,
                RowChange::Delete { .. } => report.deleted += 1,
            }
        }
        Ok(report)
    }
}

async fn execute_conditional(session: &Session, cql: &str) -> Result<bool, DriverError> {
    let rows: Vec<Row> = select_rows(session, cql, ()).await?;
    let applied = rows
        .into_iter()
        .next()
        .and_then(|row| row.columns.into_iter().next().flatten());
    match applied {
        Some(CqlValue::Boolean(applied)) => Ok(applied),
        _ => Err(DriverError::new(
            ErrorKind::Protocol,
            "conditional statement returned no [applied] column",
        )),
    }
}

fn operation(change: &RowChange) -> &'static str {
    match change {
        RowChange::Insert { .. } => "insert",
        RowChange::Update { .. } => "update",
        RowChange::Delete { .. } => "delete",
    }
}

fn saved_note(index: usize) -> String {
    format!(
        "changes are applied one by one without a transaction: {index} earlier change(s) \
         of this set were saved"
    )
}

fn not_applied(index: usize, change: &RowChange) -> DriverError {
    let reason = match change {
        RowChange::Insert { .. } => "a row with this primary key already exists",
        RowChange::Update { .. } => "the row was modified or removed by someone else",
        RowChange::Delete { .. } => "the row no longer exists",
    };
    DriverError::new(
        ErrorKind::ConcurrentModification,
        format!(
            "change #{} ({}) was not applied: {reason}",
            index + 1,
            operation(change)
        ),
    )
    .with_detail(saved_note(index))
}

fn with_saved_note(error: DriverError, index: usize) -> DriverError {
    let note = saved_note(index);
    let detail = match &error.detail {
        Some(detail) => format!("{detail}; {note}"),
        None => note,
    };
    error.with_detail(detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_changes_explain_what_was_saved() {
        let error = not_applied(2, &RowChange::Delete { key: vec![] });
        assert_eq!(error.kind, ErrorKind::ConcurrentModification);
        assert_eq!(
            error.message,
            "change #3 (delete) was not applied: the row no longer exists"
        );
        assert!(error.detail.unwrap().contains("2 earlier change(s)"));
        let failed = with_saved_note(DriverError::query("boom").with_detail("server"), 0);
        assert!(failed.detail.unwrap().starts_with("server; "));
    }
}
