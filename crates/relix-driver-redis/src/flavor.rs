/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::DriverId;
use relix_driver_api::ServerInfo;

const SERVER_NAME_KEY: &str = "server.server_name";
const REDIS_VERSION_KEY: &str = "server.redis_version";
const VALKEY_VERSION_KEY: &str = "server.valkey_version";
const VALKEY_SERVER_NAME: &str = "valkey";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flavor {
    Redis,
    Valkey,
}

impl Flavor {
    pub fn driver_id(self) -> &'static str {
        match self {
            Self::Redis => DriverId::REDIS,
            Self::Valkey => DriverId::VALKEY,
        }
    }

    pub fn product_name(self) -> &'static str {
        match self {
            Self::Redis => "Redis",
            Self::Valkey => "Valkey",
        }
    }

    /// Detects the server from parsed `INFO server` entries. Valkey keeps reporting a
    /// Redis-compatible `redis_version` and identifies itself with `server_name` and
    /// `valkey_version`.
    pub fn detect(details: &[(String, String)]) -> Self {
        let is_valkey = details.iter().any(|(key, value)| {
            (key == SERVER_NAME_KEY && value.eq_ignore_ascii_case(VALKEY_SERVER_NAME))
                || key == VALKEY_VERSION_KEY
        });
        if is_valkey { Self::Valkey } else { Self::Redis }
    }

    pub fn version(self, details: &[(String, String)]) -> Option<String> {
        let key = match self {
            Self::Redis => REDIS_VERSION_KEY,
            Self::Valkey => VALKEY_VERSION_KEY,
        };
        details
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
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
    use crate::server_details::parse_info;

    const REDIS: &str = "# Server\r\nredis_version:7.4.1\r\nredis_mode:standalone\r\n";
    const VALKEY: &str = "# Server\r\nredis_version:7.2.4\r\nserver_name:valkey\r\n\
                          valkey_version:8.1.4\r\nredis_mode:standalone\r\n";

    #[test]
    fn detects_valkey_from_info_server() {
        assert_eq!(Flavor::detect(&parse_info(REDIS)), Flavor::Redis);
        assert_eq!(Flavor::detect(&parse_info(VALKEY)), Flavor::Valkey);
        let version_only = parse_info("# Server\r\nvalkey_version:8.0.0\r\n");
        assert_eq!(Flavor::detect(&version_only), Flavor::Valkey);
    }

    #[test]
    fn version_follows_the_server() {
        assert_eq!(
            Flavor::Valkey.version(&parse_info(VALKEY)).as_deref(),
            Some("8.1.4")
        );
        assert_eq!(
            Flavor::Redis.version(&parse_info(REDIS)).as_deref(),
            Some("7.4.1")
        );
        assert_eq!(Flavor::Redis.version(&parse_info("# Clients\r\n")), None);
    }

    #[test]
    fn mismatched_driver_names_the_actual_server() {
        assert_eq!(
            Flavor::Valkey.server_info(Flavor::Valkey, "8.1.4").product,
            "Valkey"
        );
        assert_eq!(
            Flavor::Redis.server_info(Flavor::Valkey, "8.1.4").product,
            "Valkey (connected via Redis driver)"
        );
    }
}
