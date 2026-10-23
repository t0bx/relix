/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::cypher::fetch_all;
use crate::row_reader::optional_text;
use neo4rs::{Graph, query};
use relix_driver_api::{DriverError, ServerInfo};

const COMPONENTS_CYPHER: &str = "CALL dbms.components() YIELD name, versions, edition \
     WHERE name = 'Neo4j Kernel' RETURN versions[0] AS version, edition";
const CURRENT_DATABASE_CYPHER: &str = "CALL db.info() YIELD name RETURN name";

pub(crate) async fn server_info(graph: &Graph) -> Result<ServerInfo, DriverError> {
    let rows = fetch_all(graph, None, query(COMPONENTS_CYPHER)).await?;
    let (version, edition) = match rows.first() {
        Some(row) => (
            optional_text(row, "version")?,
            optional_text(row, "edition")?,
        ),
        None => (None, None),
    };
    let version = version.unwrap_or_else(|| "unknown".to_owned());
    Ok(ServerInfo {
        product: "Neo4j".to_owned(),
        version: match edition {
            Some(edition) => format!("{version} ({edition})"),
            None => version,
        },
    })
}

pub(crate) async fn current_database(graph: &Graph) -> Result<Option<String>, DriverError> {
    let rows = fetch_all(graph, None, query(CURRENT_DATABASE_CYPHER)).await?;
    match rows.first() {
        Some(row) => optional_text(row, "name"),
        None => Ok(None),
    }
}
