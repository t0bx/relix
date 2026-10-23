/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::changes::apply_change_set;
use crate::connection::SqliteConnection;
use crate::errors::map_error;
use crate::rows::read_batch;
use async_trait::async_trait;
use relix_core::{ChangeReport, ChangeSet, Dialect, RowBatch, TableQuery, TableRef};
use relix_driver_api::{DriverError, ParamStyle, SqlBuilder, TableDataAccess};

fn sqlite_builder() -> SqlBuilder {
    SqlBuilder::new(Dialect::Sqlite, ParamStyle::QuestionMark)
}

#[async_trait]
impl TableDataAccess for SqliteConnection {
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        let sql = sqlite_builder().select_page(table, query);
        let limit = query.limit as usize;
        self.shared
            .run(move |connection| {
                let mut statement = connection.prepare(&sql).map_err(map_error)?;
                read_batch(&mut statement, [], limit)
            })
            .await
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        let sql = sqlite_builder().count(table, filter);
        self.shared
            .run(move |connection| {
                let count: i64 = connection
                    .query_row(&sql, [], |row| row.get(0))
                    .map_err(map_error)?;
                Ok(u64::try_from(count).unwrap_or_default())
            })
            .await
    }

    async fn apply_changes(&self, changes: &ChangeSet) -> Result<ChangeReport, DriverError> {
        if changes.is_empty() {
            return Ok(ChangeReport::default());
        }
        let changes = changes.clone();
        self.shared
            .run(move |connection| apply_change_set(connection, sqlite_builder(), &changes))
            .await
    }
}
