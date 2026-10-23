/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{ConnectionConfig, TlsMode};
use relix_driver_api::DriverError;
use relix_driver_http::HttpAuth;

const FALLBACK_USER: &str = "relix";

/// Trino identifies every request by `X-Trino-User`; a password is sent as HTTP Basic
/// credentials, which Trino only accepts over HTTPS. A password therefore upgrades the
/// `prefer` TLS mode to `require` (never falling back to plain HTTP) and is rejected
/// when TLS is disabled.
pub(crate) fn secured_config(config: &ConnectionConfig) -> Result<ConnectionConfig, DriverError> {
    let mut secured = config.clone();
    if config.password.is_none() {
        return Ok(secured);
    }
    match config.settings.tls.mode {
        TlsMode::Disable => Err(DriverError::configuration(
            "Trino accepts passwords only over HTTPS: enable TLS or remove the password",
        )),
        TlsMode::Prefer => {
            secured.settings.tls.mode = TlsMode::Require;
            Ok(secured)
        }
        TlsMode::Require | TlsMode::VerifyFull => Ok(secured),
    }
}

pub(crate) fn user_name(config: &ConnectionConfig) -> String {
    config
        .settings
        .username
        .as_deref()
        .map(str::trim)
        .filter(|user| !user.is_empty())
        .map(str::to_owned)
        .or_else(|| std::env::var("USER").ok().filter(|user| !user.is_empty()))
        .unwrap_or_else(|| FALLBACK_USER.to_owned())
}

pub(crate) fn http_auth(config: &ConnectionConfig, user: &str) -> HttpAuth {
    match &config.password {
        Some(password) => HttpAuth::Basic {
            user: user.to_owned(),
            password: Some(password.clone()),
        },
        None => HttpAuth::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::{ConnectionSettings, Secret};
    use relix_driver_api::ErrorKind;

    fn config(mode: TlsMode, password: Option<&str>) -> ConnectionConfig {
        let mut settings = ConnectionSettings::default();
        settings.tls.mode = mode;
        ConnectionConfig::new(settings, password.map(Secret::new))
    }

    #[test]
    fn passwords_require_https() {
        let error = secured_config(&config(TlsMode::Disable, Some("pw")))
            .err()
            .unwrap();
        assert_eq!(error.kind, ErrorKind::Configuration);
        let upgraded = secured_config(&config(TlsMode::Prefer, Some("pw"))).unwrap();
        assert_eq!(upgraded.settings.tls.mode, TlsMode::Require);
        let plain = secured_config(&config(TlsMode::Disable, None)).unwrap();
        assert_eq!(plain.settings.tls.mode, TlsMode::Disable);
    }

    #[test]
    fn basic_credentials_are_only_built_with_a_password() {
        assert!(matches!(
            http_auth(&config(TlsMode::Require, Some("pw")), "ana"),
            HttpAuth::Basic { .. }
        ));
        assert!(matches!(
            http_auth(&config(TlsMode::Disable, None), "ana"),
            HttpAuth::None
        ));
    }
}
