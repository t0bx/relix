/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::catalog;
use crate::api::models::TableField;
use crate::connection::BigQueryConnection;
use crate::result::{PageCollector, page_size, query_rows, row_batch};
use async_trait::async_trait;
use relix_core::{ChangeReport, ChangeSet, RowBatch, TableQuery, TableRef, Value};
use relix_driver_api::{DriverError, ErrorKind, TableDataAccess};

const STORED_TABLE: &str = "TABLE";

#[async_trait]
impl TableDataAccess for BigQueryConnection {
    /// Unfiltered, unsorted pages of stored tables are read with `tabledata.list`, which
    /// costs nothing, instead of a `SELECT *` that BigQuery would bill as a full scan.
    /// Views, filters and sorting go through a query built by the SQL builder.
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        let limit = query.limit as usize;
        let (project, dataset) = self.table_scope(table)?;
        let unfiltered = query
            .filter
            .as_deref()
            .is_none_or(|filter| filter.trim().is_empty());
        if unfiltered && query.sort.is_empty() {
            let metadata = catalog::table(self.client(), &project, &dataset, &table.name).await?;
            if metadata.table_type.as_deref() == Some(STORED_TABLE) {
                let fields = metadata.schema.unwrap_or_default().fields;
                let path = [project.as_str(), dataset.as_str(), table.name.as_str()];
                return self.stored_rows(path, &fields, query.offset, limit).await;
            }
        }
        let qualified = TableRef::new(Some(project), Some(dataset), table.name.clone());
        let sql = self.builder.select_page(&qualified, query);
        let results = self.query(&sql, page_size(limit + 1)).await?;
        let mut batch = query_rows(self.client(), results, limit + 1).await?;
        batch.has_more = batch.rows.len() > limit;
        batch.rows.truncate(limit);
        Ok(batch)
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        let (project, dataset) = self.table_scope(table)?;
        let qualified = TableRef::new(Some(project), Some(dataset), table.name.clone());
        let results = self
            .query(&self.builder.count(&qualified, filter), 1)
            .await?;
        let batch = query_rows(self.client(), results, 1).await?;
        match batch.rows.first().and_then(|row| row.first()) {
            Some(Value::Int(count)) => u64::try_from(*count)
                .map_err(|_| DriverError::new(ErrorKind::Protocol, "negative row count")),
            _ => Err(DriverError::new(
                ErrorKind::Protocol,
                "COUNT(*) returned no number",
            )),
        }
    }

    async fn apply_changes(&self, _changes: &ChangeSet) -> Result<ChangeReport, DriverError> {
        Err(
            DriverError::unsupported("BigQuery tables are read-only in the data grid").with_detail(
                "BigQuery does not enforce primary keys, so guarded single-row changes cannot \
                 be guaranteed; change data with DML statements in the editor",
            ),
        )
    }
}

impl BigQueryConnection {
    async fn stored_rows(
        &self,
        path: [&str; 3],
        fields: &[TableField],
        offset: u64,
        limit: usize,
    ) -> Result<RowBatch, DriverError> {
        let wanted = limit + 1;
        let client = self.client();
        let first =
            catalog::table_data(client, path, Some(offset), None, page_size(wanted)).await?;
        let mut pages = PageCollector::new(first.rows, first.page_token, wanted);
        while let Some(token) = pages.next_token() {
            let page =
                catalog::table_data(client, path, None, Some(&token), page_size(wanted)).await?;
            pages.add_page(page.rows, page.page_token);
        }
        let collected = pages.finish();
        let has_more = collected.has_more || collected.rows.len() > limit;
        let rows = &collected.rows[..collected.rows.len().min(limit)];
        Ok(row_batch(fields, rows, has_more))
    }
}
