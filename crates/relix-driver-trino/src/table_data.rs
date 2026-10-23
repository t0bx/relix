/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::text_at;
use crate::connection::TrinoConnection;
use async_trait::async_trait;
use relix_core::{ChangeReport, ChangeSet, Dialect, RowBatch, TableQuery, TableRef};
use relix_driver_api::{DriverError, ParamStyle, SqlBuilder, TableDataAccess};

impl TrinoConnection {
    fn qualified_ref(&self, table: &TableRef) -> Result<TableRef, DriverError> {
        let qualified = self.qualify(table)?;
        Ok(TableRef::new(
            Some(qualified.catalog),
            Some(qualified.schema),
            qualified.name,
        ))
    }
}

fn trino_builder() -> SqlBuilder {
    SqlBuilder::new(Dialect::Trino, ParamStyle::Inline)
}

#[async_trait]
impl TableDataAccess for TrinoConnection {
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        let sql = trino_builder().select_page(&self.qualified_ref(table)?, query);
        let outcome = self.query(&sql, query.limit as usize).await?;
        Ok(outcome.batch.unwrap_or_default())
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        let sql = trino_builder().count(&self.qualified_ref(table)?, filter);
        let batch = self.query_rows(&sql).await?;
        let count = batch
            .rows
            .first()
            .map(|row| text_at(row, 0))
            .unwrap_or_default();
        count
            .parse()
            .map_err(|_| DriverError::internal(format!("unexpected row count \"{count}\"")))
    }

    async fn apply_changes(&self, _changes: &ChangeSet) -> Result<ChangeReport, DriverError> {
        Err(DriverError::unsupported(
            "Trino tables cannot be edited in the grid: connectors do not offer guarded \
             single-row updates inside one transaction",
        ))
    }
}
