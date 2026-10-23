/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::cql_rows::select_rows;
use crate::flavor::Flavor;
use relix_driver_api::{DriverError, ServerInfo};
use scylla::client::session::Session;

const RELEASE_VERSION_CQL: &str = "SELECT release_version FROM system.local WHERE key = 'local'";
const SCYLLA_VERSION_CQL: &str = "SELECT version FROM system.versions WHERE key = 'local'";

/// Identifies the server behind the session. Both products report a Cassandra
/// compatible `release_version`; only ScyllaDB has the `system.versions` table with its
/// own version, so its presence decides the product independent of the chosen driver.
pub(crate) async fn detect_server(session: &Session) -> Result<ServerInfo, DriverError> {
    let release = single_text(session, RELEASE_VERSION_CQL)
        .await?
        .unwrap_or_else(|| "unknown".to_owned());
    let server_info = match single_text(session, SCYLLA_VERSION_CQL).await {
        Ok(Some(version)) => ServerInfo {
            product: Flavor::ScyllaDb.display_name().to_owned(),
            version,
        },
        _ => ServerInfo {
            product: Flavor::Cassandra.display_name().to_owned(),
            version: release,
        },
    };
    Ok(server_info)
}

async fn single_text(session: &Session, cql: &str) -> Result<Option<String>, DriverError> {
    let rows: Vec<(Option<String>,)> = select_rows(session, cql, ()).await?;
    Ok(rows.into_iter().next().and_then(|(text,)| text))
}
