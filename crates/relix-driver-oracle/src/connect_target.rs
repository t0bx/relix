/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{ConnectionConfig, TlsMode};
use relix_driver_api::{DriverError, ErrorKind};

pub const DEFAULT_PORT: u16 = 1521;
const CLIENT_LIB_DIR_PROPERTY: &str = "client_lib_dir";

#[derive(Clone, PartialEq, Eq)]
pub struct ConnectTarget {
    pub username: String,
    pub password: String,
    pub connect_string: String,
    pub service_name: String,
    pub client_lib_dir: Option<String>,
}

/// Validates the settings and builds the Easy Connect string `//host:port/service`. The
/// service name comes from the database field. TCPS needs an Oracle wallet configured
/// outside RELIX, so only plaintext Oracle Net is offered: `Prefer` falls back to it and
/// `Require`/`VerifyFull` are rejected instead of silently connecting unencrypted.
pub fn connect_target(config: &ConnectionConfig) -> Result<ConnectTarget, DriverError> {
    let settings = &config.settings;
    if matches!(settings.tls.mode, TlsMode::Require | TlsMode::VerifyFull) {
        return Err(DriverError::new(
            ErrorKind::Tls,
            format!(
                "TLS mode {} is not supported for Oracle: TCPS needs an Oracle wallet, which \
                 RELIX does not configure; use an SSH tunnel or server-side Oracle Native \
                 Network Encryption and set TLS to disable or prefer",
                settings.tls.mode.label()
            ),
        ));
    }
    let username = non_empty(settings.username.as_deref())
        .ok_or_else(|| DriverError::configuration("a user name is required"))?;
    let service_name = non_empty(settings.database.as_deref()).ok_or_else(|| {
        DriverError::configuration("the service name (database field, e.g. FREEPDB1) is required")
    })?;
    let host = config.host_or_default();
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    Ok(ConnectTarget {
        connect_string: format!(
            "//{host}:{}/{service_name}",
            settings.port.unwrap_or(DEFAULT_PORT)
        ),
        username: username.to_owned(),
        password: config
            .password
            .as_ref()
            .map(|secret| secret.expose().to_owned())
            .unwrap_or_default(),
        service_name: service_name.to_owned(),
        client_lib_dir: settings
            .property(CLIENT_LIB_DIR_PROPERTY)
            .map(str::to_owned),
    })
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use relix_core::{ConnectionSettings, Secret};

    fn config(settings: ConnectionSettings) -> ConnectionConfig {
        ConnectionConfig::new(settings, Some(Secret::new("pw")))
    }

    fn settings() -> ConnectionSettings {
        ConnectionSettings {
            host: Some("db.internal".into()),
            username: Some("relix".into()),
            database: Some("FREEPDB1".into()),
            ..ConnectionSettings::default()
        }
    }

    #[test]
    fn easy_connect_string_uses_host_port_and_service() {
        let target = connect_target(&config(settings())).unwrap();
        assert_eq!(target.connect_string, "//db.internal:1521/FREEPDB1");
        assert_eq!(target.username, "relix");
        let tunneled = config(settings()).redirected("::1", 40000);
        assert_eq!(
            connect_target(&tunneled).unwrap().connect_string,
            "//[::1]:40000/FREEPDB1"
        );
    }

    #[test]
    fn missing_user_or_service_is_a_configuration_error() {
        let mut without_service = settings();
        without_service.database = Some(" ".into());
        assert_eq!(
            connect_target(&config(without_service)).err().unwrap().kind,
            ErrorKind::Configuration
        );
        let mut without_user = settings();
        without_user.username = None;
        assert_eq!(
            connect_target(&config(without_user)).err().unwrap().kind,
            ErrorKind::Configuration
        );
    }

    #[test]
    fn enforced_tls_is_rejected() {
        for mode in [TlsMode::Require, TlsMode::VerifyFull] {
            let mut enforced = settings();
            enforced.tls.mode = mode;
            assert_eq!(
                connect_target(&config(enforced)).err().unwrap().kind,
                ErrorKind::Tls
            );
        }
        let mut preferred = settings();
        preferred.tls.mode = TlsMode::Prefer;
        preferred
            .properties
            .insert(CLIENT_LIB_DIR_PROPERTY.into(), "/opt/oracle".into());
        let target = connect_target(&config(preferred)).unwrap();
        assert_eq!(target.client_lib_dir.as_deref(), Some("/opt/oracle"));
    }
}
