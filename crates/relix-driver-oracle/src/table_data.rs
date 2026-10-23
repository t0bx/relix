/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::changes::apply_change_set;
use crate::connection::{OracleConnection, sql_builder};
use crate::errors::map_error;
use crate::result_rows::read_rows;
use async_trait::async_trait;
use relix_core::{ChangeReport, ChangeSet, RowBatch, TableQuery, TableRef};
use relix_driver_api::{DriverError, TableDataAccess};

#[async_trait]
impl TableDataAccess for OracleConnection {
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        let sql = sql_builder().select_page(&self.resolve_table(table), query);
        let limit = usize::try_from(query.limit).unwrap_or(usize::MAX);
        self.data
            .run(move |connection| {
                let rows = connection.query(&sql, &[]).map_err(map_error)?;
                read_rows(rows, limit).map_err(map_error)
            })
            .await
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        let sql = sql_builder().count(&self.resolve_table(table), filter);
        self.data
            .run(move |connection| connection.query_row_as::<u64>(&sql, &[]).map_err(map_error))
            .await
    }

    async fn apply_changes(&self, changes: &ChangeSet) -> Result<ChangeReport, DriverError> {
        if changes.is_empty() {
            return Ok(ChangeReport::default());
        }
        let table = self.resolve_table(&changes.table);
        let changes = changes.changes.clone();
        self.data
            .run(move |connection| apply_change_set(connection, &table, &changes))
            .await
    }
}
