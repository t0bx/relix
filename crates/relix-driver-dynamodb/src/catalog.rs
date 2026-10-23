/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::DynamoConnection;
use crate::errors::map_sdk_error;
use futures_util::{StreamExt, stream};
use relix_core::{CollectionInfo, CollectionKind, CollectionRef, DatabaseInfo};
use relix_driver_api::DriverError;

const CONCURRENT_DESCRIBES: usize = 8;
const MAX_DESCRIBED_TABLES: usize = 100;

impl DynamoConnection {
    pub(crate) fn list_namespaces(&self) -> Vec<DatabaseInfo> {
        vec![DatabaseInfo {
            name: self.region.clone(),
            is_default: true,
        }]
    }

    /// Lists every table of the region, following `LastEvaluatedTableName`. Item counts
    /// (DynamoDB's approximation, refreshed about every six hours) are fetched with
    /// `DescribeTable` only for regions with at most a hundred tables to keep browsing
    /// fast and within the control-plane rate limits.
    pub(crate) async fn list_tables(&self) -> Result<Vec<CollectionInfo>, DriverError> {
        let client = self.client()?;
        let mut names = Vec::new();
        let mut start = None;
        loop {
            let output = client
                .list_tables()
                .set_exclusive_start_table_name(start)
                .send()
                .await
                .map_err(map_sdk_error)?;
            names.extend(output.table_names.unwrap_or_default());
            start = output.last_evaluated_table_name;
            if start.is_none() {
                break;
            }
        }
        let describe = names.len() <= MAX_DESCRIBED_TABLES;
        let tables = stream::iter(names)
            .map(|name| self.table_entry(name, describe))
            .buffered(CONCURRENT_DESCRIBES)
            .collect()
            .await;
        Ok(tables)
    }

    async fn table_entry(&self, name: String, describe: bool) -> CollectionInfo {
        let description = match describe {
            true => self.describe_table(&name).await.ok(),
            false => None,
        };
        CollectionInfo {
            collection: CollectionRef::new(self.region.clone(), name),
            kind: CollectionKind::Table,
            document_count: description
                .as_ref()
                .and_then(|table| table.item_count)
                .and_then(|count| u64::try_from(count).ok()),
            detail: description
                .as_ref()
                .and_then(|table| table.table_status.as_ref())
                .map(|status| status.as_str().to_lowercase()),
        }
    }
}
