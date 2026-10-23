/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::client::BigQueryClient;
use crate::api::models::{
    DatasetEntry, DatasetList, RoutineEntry, RoutineList, Table, TableDataPage, TableEntry,
    TableList,
};
use relix_driver_api::DriverError;
use serde::de::DeserializeOwned;

const LIST_PAGE_SIZE: &str = "1000";

trait Listing: DeserializeOwned {
    type Item;

    fn into_parts(self) -> (Vec<Self::Item>, Option<String>);
}

impl Listing for DatasetList {
    type Item = DatasetEntry;

    fn into_parts(self) -> (Vec<DatasetEntry>, Option<String>) {
        (self.datasets, self.next_page_token)
    }
}

impl Listing for TableList {
    type Item = TableEntry;

    fn into_parts(self) -> (Vec<TableEntry>, Option<String>) {
        (self.tables, self.next_page_token)
    }
}

impl Listing for RoutineList {
    type Item = RoutineEntry;

    fn into_parts(self) -> (Vec<RoutineEntry>, Option<String>) {
        (self.routines, self.next_page_token)
    }
}

pub(crate) async fn datasets(
    client: &BigQueryClient,
    project: &str,
) -> Result<Vec<DatasetEntry>, DriverError> {
    list_all::<DatasetList>(client, &["projects", project, "datasets"]).await
}

pub(crate) async fn first_dataset_page(
    client: &BigQueryClient,
    project: &str,
) -> Result<(), DriverError> {
    client
        .get::<DatasetList>(
            &["projects", project, "datasets"],
            &[("maxResults", "1".to_owned())],
            None,
        )
        .await
        .map(|_| ())
}

pub(crate) async fn tables(
    client: &BigQueryClient,
    project: &str,
    dataset: &str,
) -> Result<Vec<TableEntry>, DriverError> {
    list_all::<TableList>(
        client,
        &["projects", project, "datasets", dataset, "tables"],
    )
    .await
}

pub(crate) async fn routines(
    client: &BigQueryClient,
    project: &str,
    dataset: &str,
) -> Result<Vec<RoutineEntry>, DriverError> {
    list_all::<RoutineList>(
        client,
        &["projects", project, "datasets", dataset, "routines"],
    )
    .await
}

pub(crate) async fn table(
    client: &BigQueryClient,
    project: &str,
    dataset: &str,
    table: &str,
) -> Result<Table, DriverError> {
    client
        .get(
            &["projects", project, "datasets", dataset, "tables", table],
            &[],
            None,
        )
        .await
}

/// Reads stored rows with `tabledata.list`, which is free of query charges.
pub(crate) async fn table_data(
    client: &BigQueryClient,
    path: [&str; 3],
    start_index: Option<u64>,
    page_token: Option<&str>,
    max_results: u32,
) -> Result<TableDataPage, DriverError> {
    let [project, dataset, table] = path;
    let mut query = vec![
        ("maxResults", max_results.to_string()),
        ("formatOptions.useInt64Timestamp", "true".to_owned()),
    ];
    match page_token {
        Some(token) => query.push(("pageToken", token.to_owned())),
        None => query.extend(start_index.map(|start| ("startIndex", start.to_string()))),
    }
    client
        .get(
            &[
                "projects", project, "datasets", dataset, "tables", table, "data",
            ],
            &query,
            None,
        )
        .await
}

async fn list_all<L: Listing>(
    client: &BigQueryClient,
    segments: &[&str],
) -> Result<Vec<L::Item>, DriverError> {
    let mut items = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut query = vec![("maxResults", LIST_PAGE_SIZE.to_owned())];
        query.extend(token.take().map(|token| ("pageToken", token)));
        let (page, next) = client.get::<L>(segments, &query, None).await?.into_parts();
        items.extend(page);
        match next.filter(|next| !next.is_empty()) {
            Some(next) => token = Some(next),
            None => return Ok(items),
        }
    }
}
