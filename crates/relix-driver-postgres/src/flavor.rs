/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::DriverId;
use relix_driver_api::ServerInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flavor {
    Postgres,
    CockroachDb,
    YugabyteDb,
    TimescaleDb,
}

impl Flavor {
    pub fn driver_id(self) -> &'static str {
        match self {
            Self::Postgres => DriverId::POSTGRES,
            Self::CockroachDb => DriverId::COCKROACHDB,
            Self::YugabyteDb => DriverId::YUGABYTEDB,
            Self::TimescaleDb => DriverId::TIMESCALEDB,
        }
    }

    pub fn product_name(self) -> &'static str {
        match self {
            Self::Postgres => "PostgreSQL",
            Self::CockroachDb => "CockroachDB",
            Self::YugabyteDb => "YugabyteDB",
            Self::TimescaleDb => "TimescaleDB",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Self::Postgres | Self::TimescaleDb => 5432,
            Self::CockroachDb => 26257,
            Self::YugabyteDb => 5433,
        }
    }

    pub(crate) fn default_user(self) -> &'static str {
        match self {
            Self::Postgres | Self::TimescaleDb => "postgres",
            Self::CockroachDb => "root",
            Self::YugabyteDb => "yugabyte",
        }
    }

    /// The database used when none is configured; `None` means "named like the user",
    /// which is the libpq convention. CockroachDB has no per-user databases.
    pub(crate) fn default_database(self) -> Option<&'static str> {
        match self {
            Self::CockroachDb => Some("defaultdb"),
            Self::Postgres | Self::YugabyteDb | Self::TimescaleDb => None,
        }
    }

    /// Detects the engine behind a PostgreSQL wire connection from `version()` and the
    /// installed `timescaledb` extension of the connected database.
    pub fn detect(full_version: &str, timescale_installed: bool) -> Self {
        if full_version.contains("CockroachDB") {
            Self::CockroachDb
        } else if full_version.contains("-YB-") {
            Self::YugabyteDb
        } else if timescale_installed {
            Self::TimescaleDb
        } else {
            Self::Postgres
        }
    }

    pub(crate) fn may_have_timescale_catalog(self) -> bool {
        matches!(self, Self::Postgres | Self::TimescaleDb)
    }

    pub(crate) fn system_schemas(self) -> &'static [&'static str] {
        match self {
            Self::CockroachDb => &[
                "pg_catalog",
                "information_schema",
                "crdb_internal",
                "pg_extension",
            ],
            Self::Postgres | Self::YugabyteDb | Self::TimescaleDb => {
                &["pg_catalog", "information_schema"]
            }
        }
    }

    pub fn server_info(self, server: Self, version: &str) -> ServerInfo {
        let product = if server == self {
            server.product_name().to_owned()
        } else {
            format!(
                "{} (connected via {} driver)",
                server.product_name(),
                self.product_name()
            )
        };
        ServerInfo {
            product,
            version: version.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_engines_from_version_and_extensions() {
        assert_eq!(
            Flavor::detect(
                "CockroachDB CCL v25.2.1 (aarch64-unknown-linux-gnu, built 2025/06/02)",
                false
            ),
            Flavor::CockroachDb
        );
        assert_eq!(
            Flavor::detect(
                "PostgreSQL 15.12-YB-2.25.2.0-b0 on aarch64-unknown-linux-gnu",
                false
            ),
            Flavor::YugabyteDb
        );
        assert_eq!(
            Flavor::detect("PostgreSQL 17.6 on aarch64-unknown-linux-musl", true),
            Flavor::TimescaleDb
        );
        assert_eq!(
            Flavor::detect("PostgreSQL 17.6 on aarch64-unknown-linux-musl", false),
            Flavor::Postgres
        );
    }

    #[test]
    fn mismatched_driver_names_the_actual_engine() {
        assert_eq!(
            Flavor::CockroachDb
                .server_info(Flavor::CockroachDb, "v25.2.1")
                .product,
            "CockroachDB"
        );
        assert_eq!(
            Flavor::Postgres
                .server_info(Flavor::YugabyteDb, "2.25")
                .product,
            "YugabyteDB (connected via PostgreSQL driver)"
        );
    }

    #[test]
    fn defaults_follow_the_engine() {
        assert_eq!(Flavor::CockroachDb.default_port(), 26257);
        assert_eq!(Flavor::YugabyteDb.default_port(), 5433);
        assert_eq!(Flavor::TimescaleDb.default_port(), 5432);
        assert_eq!(Flavor::CockroachDb.default_database(), Some("defaultdb"));
        assert_eq!(Flavor::YugabyteDb.default_user(), "yugabyte");
    }
}
