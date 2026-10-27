/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::DriverId;
use relix_driver_api::ServerInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flavor {
    MySql,
    MariaDb,
    TiDb,
}

const TIDB_MARKER: &str = "-TiDB-";

impl Flavor {
    pub fn driver_id(self) -> &'static str {
        match self {
            Self::MySql => DriverId::MYSQL,
            Self::MariaDb => DriverId::MARIADB,
            Self::TiDb => DriverId::TIDB,
        }
    }

    pub fn product_name(self) -> &'static str {
        match self {
            Self::MySql => "MySQL",
            Self::MariaDb => "MariaDB",
            Self::TiDb => "TiDB",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Self::MySql | Self::MariaDb => 3306,
            Self::TiDb => 4000,
        }
    }

    /// TiDB parses `CREATE PROCEDURE`/`FUNCTION` but does not support stored routines.
    pub fn supports_routines(self) -> bool {
        self != Self::TiDb
    }

    /// TiDB rejects `SET TRANSACTION READ ONLY` unless noop functions are enabled.
    pub fn supports_read_only_sessions(self) -> bool {
        self != Self::TiDb
    }

    pub fn detect(version: &str) -> Self {
        let lowercase = version.to_ascii_lowercase();
        if lowercase.contains("tidb") {
            Self::TiDb
        } else if lowercase.contains("mariadb") {
            Self::MariaDb
        } else {
            Self::MySql
        }
    }

    /// TiDB reports `<mysql-level>-TiDB-<release>`; its own release comes first.
    fn release_version(self, version: &str) -> String {
        match (self, version.split_once(TIDB_MARKER)) {
            (Self::TiDb, Some((mysql, release))) => {
                format!("{release} (MySQL {mysql} compatible)")
            }
            _ => version.to_owned(),
        }
    }

    pub fn server_info(self, version: &str) -> ServerInfo {
        let server = Self::detect(version);
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
            version: server.release_version(version),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_server_product_from_version_string() {
        assert_eq!(Flavor::detect("8.4.3"), Flavor::MySql);
        assert_eq!(Flavor::detect("11.4.2-MariaDB-ubu2404"), Flavor::MariaDb);
        assert_eq!(Flavor::detect("5.5.5-10.11.6-mariadb-log"), Flavor::MariaDb);
        assert_eq!(Flavor::detect("8.0.11-TiDB-v8.5.3"), Flavor::TiDb);
    }

    #[test]
    fn tidb_reports_its_release_first() {
        let info = Flavor::TiDb.server_info("8.0.11-TiDB-v8.5.3");
        assert_eq!(info.product, "TiDB");
        assert_eq!(info.version, "v8.5.3 (MySQL 8.0.11 compatible)");
        assert_eq!(
            Flavor::MySql.server_info("8.0.11-TiDB-v8.5.3").product,
            "TiDB (connected via MySQL driver)"
        );
        assert_eq!(Flavor::TiDb.default_port(), 4000);
        assert!(!Flavor::TiDb.supports_routines());
    }

    #[test]
    fn matching_flavor_reports_plain_product() {
        let info = Flavor::MySql.server_info("8.4.3");
        assert_eq!(info.product, "MySQL");
        assert_eq!(info.version, "8.4.3");
        assert_eq!(
            Flavor::MariaDb.server_info("11.4.2-MariaDB").product,
            "MariaDB"
        );
    }

    #[test]
    fn mismatched_flavor_is_reported() {
        assert_eq!(
            Flavor::MariaDb.server_info("8.4.3").product,
            "MySQL (connected via MariaDB driver)"
        );
        assert_eq!(
            Flavor::MySql.server_info("11.4.2-MariaDB").product,
            "MariaDB (connected via MySQL driver)"
        );
    }
}
