/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::cypher::fetch_all;
use crate::row_reader::optional_text;
use neo4rs::{BoltMap, BoltString, BoltType, Graph, Query, query};
use relix_driver_api::DriverError;

const SESSION_KEY: &str = "relix_session";
const FIND_CYPHER: &str = "SHOW TRANSACTIONS YIELD transactionId, metaData \
     WHERE metaData.relix_session = $token RETURN transactionId";
const TERMINATE_CYPHER: &str = "TERMINATE TRANSACTIONS $ids";

/// Tags an auto-commit statement with transaction metadata naming this console
/// session, so that `terminate_session_transactions` can find it on the server.
pub(crate) fn tagged(statement: &str, token: &str) -> Query {
    let mut metadata = BoltMap::new();
    metadata.put(BoltString::new(SESSION_KEY), BoltType::from(token));
    query(statement).extra("tx_metadata", BoltType::Map(metadata))
}

/// Terminates the server transactions started by this console session. Bolt 4 cannot
/// interrupt a running request on its own connection, so cancellation asks the server
/// through `SHOW TRANSACTIONS` / `TERMINATE TRANSACTIONS` on another pooled connection.
pub(crate) async fn terminate_session_transactions(
    graph: &Graph,
    token: &str,
) -> Result<usize, DriverError> {
    let rows = fetch_all(graph, None, query(FIND_CYPHER).param("token", token)).await?;
    let mut ids = Vec::with_capacity(rows.len());
    for row in &rows {
        if let Some(id) = optional_text(row, "transactionId")? {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Ok(0);
    }
    let count = ids.len();
    fetch_all(graph, None, query(TERMINATE_CYPHER).param("ids", ids)).await?;
    Ok(count)
}
