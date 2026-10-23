/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client_options::{client_options, configured_database};
use crate::connection::MongoConnection;
use crate::errors::map_error;
use mongodb::Client;
use mongodb::bson::doc;
use relix_core::ConnectionConfig;
use relix_driver_api::{DriverError, ServerInfo, with_timeout};

pub const ADMIN_DATABASE: &str = "admin";
const FALLBACK_DATABASE: &str = "test";
const COMMENT_WIRE_VERSION: i32 = 9;

/// Creates the client and forces server selection and authentication with `hello` and
/// `buildInfo`, so a wrong password or unreachable host fails here instead of on the
/// first browse. Everything runs within the connect timeout.
pub async fn open_connection(config: &ConnectionConfig) -> Result<MongoConnection, DriverError> {
    with_timeout(config.settings.connect_timeout(), async {
        let options = client_options(config).await?;
        let default_database = options
            .default_database
            .clone()
            .or_else(|| configured_database(config).map(str::to_owned))
            .unwrap_or_else(|| FALLBACK_DATABASE.to_owned());
        let client = Client::with_options(options).map_err(map_error)?;
        let admin = client.database(ADMIN_DATABASE);
        let hello = admin
            .run_command(doc! { "hello": 1 })
            .await
            .map_err(map_error)?;
        let build_info = admin
            .run_command(doc! { "buildInfo": 1 })
            .await
            .map_err(map_error)?;
        let version = build_info
            .get_str("version")
            .unwrap_or("unknown")
            .to_owned();
        let supports_comment = hello
            .get_i32("maxWireVersion")
            .is_ok_and(|wire| wire >= COMMENT_WIRE_VERSION);
        Ok(MongoConnection::new(
            client,
            default_database,
            ServerInfo {
                product: "MongoDB".to_owned(),
                version,
            },
            supports_comment,
            config.settings.query_timeout(),
        ))
    })
    .await
}
