/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::flavor::Flavor;
use crate::row_reader::read_column;
use relix_driver_api::DriverError;
use tokio_postgres::Client;

const IDENTITY_SQL: &str =
    "SELECT version(), current_setting('server_version'), current_database()";

const TIMESCALE_SQL: &str =
    "SELECT extversion::text FROM pg_catalog.pg_extension WHERE extname = 'timescaledb'";

const YUGABYTE_MARKER: &str = "-YB-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerIdentity {
    pub(crate) flavor: Flavor,
    pub(crate) version: String,
    pub(crate) database: String,
}

pub(crate) async fn identify_server(client: &Client) -> Result<ServerIdentity, DriverError> {
    let row = client
        .query_one(IDENTITY_SQL, &[])
        .await
        .map_err(map_error)?;
    let full_version: String = read_column(&row, 0)?;
    let server_version: String = read_column(&row, 1)?;
    let database: String = read_column(&row, 2)?;
    let timescale_version = if full_version.contains("CockroachDB") {
        None
    } else {
        client
            .query_opt(TIMESCALE_SQL, &[])
            .await
            .map_err(map_error)?
            .map(|row| read_column::<String>(&row, 0))
            .transpose()?
    };
    let flavor = Flavor::detect(&full_version, timescale_version.is_some());
    Ok(ServerIdentity {
        flavor,
        version: product_version(
            flavor,
            &full_version,
            &server_version,
            timescale_version.as_deref(),
        ),
        database,
    })
}

/// Reports the version of the product itself, keeping the PostgreSQL compatibility
/// level in parentheses for engines that version independently of PostgreSQL.
fn product_version(
    flavor: Flavor,
    full_version: &str,
    server_version: &str,
    timescale_version: Option<&str>,
) -> String {
    match flavor {
        Flavor::Postgres => server_version.to_owned(),
        Flavor::CockroachDb => full_version
            .split_whitespace()
            .find(|word| {
                word.strip_prefix('v')
                    .is_some_and(|rest| rest.starts_with(|ch: char| ch.is_ascii_digit()))
            })
            .unwrap_or(full_version)
            .to_owned(),
        Flavor::YugabyteDb => match server_version.split_once(YUGABYTE_MARKER) {
            Some((postgres, yugabyte)) => format!("{yugabyte} (PostgreSQL {postgres})"),
            None => server_version.to_owned(),
        },
        Flavor::TimescaleDb => format!(
            "{} (PostgreSQL {server_version})",
            timescale_version.unwrap_or("unknown")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cockroach_version_is_taken_from_the_banner() {
        assert_eq!(
            product_version(
                Flavor::CockroachDb,
                "CockroachDB CCL v25.2.1 (aarch64-unknown-linux-gnu, built 2025/06/02)",
                "13.0.0",
                None
            ),
            "v25.2.1"
        );
    }

    #[test]
    fn yugabyte_and_timescale_keep_the_postgres_level() {
        assert_eq!(
            product_version(Flavor::YugabyteDb, "", "15.12-YB-2.25.2.0-b0", None),
            "2.25.2.0-b0 (PostgreSQL 15.12)"
        );
        assert_eq!(
            product_version(Flavor::TimescaleDb, "", "17.6", Some("2.22.1")),
            "2.22.1 (PostgreSQL 17.6)"
        );
        assert_eq!(
            product_version(Flavor::Postgres, "PostgreSQL 17.6 on x", "17.6", None),
            "17.6"
        );
    }
}
