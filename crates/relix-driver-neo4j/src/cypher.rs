/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::errors::map_error;
use crate::row_reader::read_rows;
use neo4rs::{DetachedRowStream, Graph, Query, Row};
use relix_driver_api::DriverError;

pub(crate) async fn open_stream(
    graph: &Graph,
    database: Option<&str>,
    query: Query,
) -> Result<DetachedRowStream, neo4rs::Error> {
    match database.filter(|name| !name.is_empty()) {
        Some(database) => graph.execute_on(database, query).await,
        None => graph.execute(query).await,
    }
}

pub(crate) async fn fetch_all(
    graph: &Graph,
    database: Option<&str>,
    query: Query,
) -> Result<Vec<Row>, DriverError> {
    let stream = open_stream(graph, database, query)
        .await
        .map_err(map_error)?;
    let (rows, _) = read_rows(stream, usize::MAX).await.map_err(map_error)?;
    Ok(rows)
}

pub(crate) fn quote(identifier: &str) -> String {
    relix_core::Dialect::Cypher.quote_identifier(identifier)
}
