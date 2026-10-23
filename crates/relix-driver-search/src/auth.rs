/*
 * @author t0bx
 * @since 23.10.2026
 */

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use relix_core::{ConnectionConfig, Secret};
use relix_driver_api::DriverError;
use relix_driver_http::HttpAuth;

/// Chooses the authentication from the `auth` property: `basic` (default when a user
/// name is set), `api_key` (the password slot holds the encoded key, or the key secret
/// when the user name holds the key id), `bearer` (the password slot holds the token)
/// or `none`.
pub fn http_auth(config: &ConnectionConfig) -> Result<HttpAuth, DriverError> {
    let mode = config
        .settings
        .property("auth")
        .map(str::to_ascii_lowercase);
    let user = config
        .settings
        .username
        .as_deref()
        .map(str::trim)
        .filter(|user| !user.is_empty());
    match mode.as_deref() {
        None => Ok(HttpAuth::basic_from(config)),
        Some("basic") => match user {
            Some(_) => Ok(HttpAuth::basic_from(config)),
            None => Err(DriverError::configuration(
                "basic authentication needs a user name",
            )),
        },
        Some("api_key" | "apikey" | "api-key") => {
            let secret = required_secret(config, "an API key")?;
            let encoded = match user {
                Some(id) => STANDARD.encode(format!("{id}:{secret}")),
                None => secret.to_owned(),
            };
            Ok(HttpAuth::Header {
                name: "Authorization".to_owned(),
                value: Secret::new(format!("ApiKey {encoded}")),
            })
        }
        Some("bearer" | "token") => Ok(HttpAuth::Bearer(Secret::new(required_secret(
            config,
            "a bearer token",
        )?))),
        Some("none") => Ok(HttpAuth::None),
        Some(other) => Err(DriverError::configuration(format!(
            "unknown auth mode \"{other}\" (use basic, api_key, bearer or none)"
        ))),
    }
}

fn required_secret<'a>(config: &'a ConnectionConfig, what: &str) -> Result<&'a str, DriverError> {
    config
        .password
        .as_ref()
        .map(|secret| secret.expose().trim())
        .filter(|secret| !secret.is_empty())
        .ok_or_else(|| DriverError::configuration(format!("enter {what} in the password field")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::ConnectionSettings;
    use relix_driver_api::ErrorKind;

    fn config(user: Option<&str>, password: Option<&str>, auth: Option<&str>) -> ConnectionConfig {
        let mut settings = ConnectionSettings {
            username: user.map(str::to_owned),
            ..ConnectionSettings::default()
        };
        if let Some(auth) = auth {
            settings.properties.insert("auth".into(), auth.into());
        }
        ConnectionConfig::new(settings, password.map(Secret::new))
    }

    fn header_value(auth: HttpAuth) -> String {
        match auth {
            HttpAuth::Header { name, value } => format!("{name}: {}", value.expose()),
            HttpAuth::Bearer(token) => format!("Bearer {}", token.expose()),
            HttpAuth::Basic { user, .. } => format!("Basic {user}"),
            HttpAuth::None => "none".into(),
        }
    }

    #[test]
    fn defaults_to_basic_with_a_user_and_none_without() {
        let basic = http_auth(&config(Some("elastic"), Some("pw"), None)).unwrap();
        assert_eq!(header_value(basic), "Basic elastic");
        let anonymous = http_auth(&config(None, None, None)).unwrap();
        assert_eq!(header_value(anonymous), "none");
    }

    #[test]
    fn api_keys_use_the_api_key_scheme() {
        let encoded = http_auth(&config(None, Some("ZW5jb2RlZA=="), Some("api_key"))).unwrap();
        assert_eq!(header_value(encoded), "Authorization: ApiKey ZW5jb2RlZA==");
        let split = http_auth(&config(Some("id"), Some("secret"), Some("API_KEY"))).unwrap();
        assert_eq!(
            header_value(split),
            format!("Authorization: ApiKey {}", STANDARD.encode("id:secret"))
        );
    }

    #[test]
    fn bearer_tokens_need_a_secret() {
        let bearer = http_auth(&config(None, Some("tok"), Some("bearer"))).unwrap();
        assert_eq!(header_value(bearer), "Bearer tok");
        let missing = http_auth(&config(None, None, Some("bearer")))
            .err()
            .unwrap();
        assert_eq!(missing.kind, ErrorKind::Configuration);
        assert!(http_auth(&config(None, None, Some("kerberos"))).is_err());
        assert!(http_auth(&config(None, Some("pw"), Some("basic"))).is_err());
    }
}
