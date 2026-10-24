/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{ConnectionConfig, Secret};
use relix_driver_api::{ConnectionProperty, DriverError};
use relix_driver_http::RequestBuilder;

pub(crate) const AUTH_METHOD: ConnectionProperty = ConnectionProperty::optional(
    "auth_method",
    "headers (default, X-ClickHouse-User/Key) or basic (HTTP Basic, for proxies)",
);
const USER_HEADER: &str = "X-ClickHouse-User";
const KEY_HEADER: &str = "X-ClickHouse-Key";

/// How the user name and password reach the server: ClickHouse's own
/// `X-ClickHouse-User`/`X-ClickHouse-Key` headers (default) or HTTP Basic, which some
/// proxies in front of ClickHouse require.
#[derive(Clone)]
pub(crate) enum Credentials {
    Headers {
        user: Option<String>,
        key: Option<Secret>,
    },
    Basic {
        user: String,
        password: Option<Secret>,
    },
}

impl Credentials {
    pub(crate) fn from_config(config: &ConnectionConfig) -> Result<Self, DriverError> {
        let user = config
            .settings
            .username
            .as_deref()
            .map(str::trim)
            .filter(|user| !user.is_empty())
            .map(str::to_owned);
        let password = config.password.clone();
        match config.settings.property(AUTH_METHOD.name) {
            None | Some("headers") => Ok(Self::Headers {
                user,
                key: password,
            }),
            Some("basic") => Ok(Self::Basic {
                user: user.unwrap_or_else(|| "default".to_owned()),
                password,
            }),
            Some(other) => Err(DriverError::configuration(format!(
                "unknown auth_method \"{other}\" (expected headers or basic)"
            ))),
        }
    }

    pub(crate) fn apply(&self, request: RequestBuilder) -> RequestBuilder {
        match self {
            Self::Headers { user, key } => {
                let request = match user {
                    Some(user) => request.header(USER_HEADER, user.as_str()),
                    None => request,
                };
                match key {
                    Some(key) => request.header(KEY_HEADER, key.expose()),
                    None => request,
                }
            }
            Self::Basic { user, password } => {
                request.basic_auth(user, password.as_ref().map(Secret::expose))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::ConnectionSettings;
    use relix_driver_api::ErrorKind;

    fn config(method: Option<&str>) -> ConnectionConfig {
        let mut settings = ConnectionSettings {
            username: Some(" analyst ".into()),
            ..ConnectionSettings::default()
        };
        if let Some(method) = method {
            settings
                .properties
                .insert(AUTH_METHOD.name.into(), method.into());
        }
        ConnectionConfig::new(settings, Some(Secret::new("pw")))
    }

    #[test]
    fn headers_are_the_default_and_basic_is_selectable() {
        assert!(matches!(
            Credentials::from_config(&config(None)),
            Ok(Credentials::Headers { user: Some(user), .. }) if user == "analyst"
        ));
        assert!(matches!(
            Credentials::from_config(&config(Some("basic"))),
            Ok(Credentials::Basic { .. })
        ));
        let error = Credentials::from_config(&config(Some("kerberos")))
            .err()
            .unwrap();
        assert_eq!(error.kind, ErrorKind::Configuration);
    }
}
