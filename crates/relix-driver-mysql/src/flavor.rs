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
}

impl Flavor {
    pub fn driver_id(self) -> &'static str {
        match self {
            Self::MySql => DriverId::MYSQL,
            Self::MariaDb => DriverId::MARIADB,
        }
    }

    pub fn product_name(self) -> &'static str {
        match self {
            Self::MySql => "MySQL",
            Self::MariaDb => "MariaDB",
        }
    }

    pub fn detect(version: &str) -> Self {
        if version.to_ascii_lowercase().contains("mariadb") {
            Self::MariaDb
        } else {
            Self::MySql
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
            version: version.to_owned(),
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
