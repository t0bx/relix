/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::account::Account;
use crate::session_context::{StatementContext, object_name};
use relix_core::{ConnectionConfig, TlsMode};
use relix_driver_api::DriverError;
use relix_driver_http::Url;
use std::path::PathBuf;

pub(crate) const DEFAULT_PORT: u16 = 443;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthMethod {
    KeyPair,
    OAuth,
    ProgrammaticAccessToken,
}

#[derive(Debug, Clone)]
pub(crate) struct SnowflakeSettings {
    pub account: Account,
    pub user: Option<String>,
    pub base_url: Url,
    pub auth: AuthMethod,
    pub private_key_file: Option<PathBuf>,
    pub context: StatementContext,
    pub statement_timeout_secs: Option<u64>,
}

impl SnowflakeSettings {
    pub(crate) fn from_config(config: &ConnectionConfig) -> Result<Self, DriverError> {
        let settings = &config.settings;
        let tunneled = config.tls_server_name.is_some();
        let configured_host = if tunneled {
            config.tls_server_name.as_deref()
        } else {
            settings.host.as_deref()
        }
        .map(str::trim)
        .filter(|host| !host.is_empty());
        let account = Account::resolve(settings.property("account"), configured_host)?;
        let host = match configured_host {
            Some("localhost") if tunneled => {
                return Err(DriverError::configuration(
                    "set the host to the account URL (<account>.snowflakecomputing.com) to \
                     reach Snowflake through an SSH tunnel",
                ));
            }
            Some(host) => host.to_owned(),
            None => account.default_host(),
        };
        Ok(Self {
            user: settings
                .username
                .as_deref()
                .map(str::trim)
                .filter(|user| !user.is_empty())
                .map(str::to_owned),
            base_url: base_url(config, &host)?,
            auth: auth_method(settings.property("authenticator"))?,
            private_key_file: settings.property("private_key_file").map(PathBuf::from),
            context: StatementContext {
                database: settings
                    .database
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .map(object_name),
                schema: settings.property("schema").map(object_name),
                warehouse: settings.property("warehouse").map(object_name),
                role: settings.property("role").map(object_name),
            },
            statement_timeout_secs: settings.query_timeout_secs.filter(|seconds| *seconds > 0),
            account,
        })
    }
}

fn base_url(config: &ConnectionConfig, host: &str) -> Result<Url, DriverError> {
    let scheme = match config.settings.tls.mode {
        TlsMode::Disable => "http",
        _ => "https",
    };
    let port = config.settings.port.unwrap_or(DEFAULT_PORT);
    let host_part = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    Url::parse(&format!("{scheme}://{host_part}:{port}/"))
        .map_err(|error| DriverError::configuration(format!("invalid account URL: {error}")))
}

fn auth_method(value: Option<&str>) -> Result<AuthMethod, DriverError> {
    let normalized = value.map(str::to_ascii_lowercase);
    match normalized.as_deref() {
        None | Some("snowflake_jwt" | "key_pair" | "jwt") => Ok(AuthMethod::KeyPair),
        Some("oauth") => Ok(AuthMethod::OAuth),
        Some("programmatic_access_token" | "pat") => Ok(AuthMethod::ProgrammaticAccessToken),
        Some("snowflake" | "password" | "username_password_mfa" | "externalbrowser") => {
            Err(DriverError::unsupported(
                "Snowflake's SQL API does not accept password or browser logins",
            )
            .with_detail(
                "use key-pair authentication (authenticator = snowflake_jwt with \
                 private_key_file), an OAuth token (authenticator = oauth) or a programmatic \
                 access token (authenticator = programmatic_access_token)",
            ))
        }
        Some(other) => Err(DriverError::configuration(format!(
            "unknown Snowflake authenticator \"{other}\""
        ))),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use relix_core::ConnectionSettings;
    use relix_driver_api::ErrorKind;

    fn config(properties: &[(&str, &str)]) -> ConnectionConfig {
        let mut settings = ConnectionSettings {
            username: Some("analyst".into()),
            database: Some("sales".into()),
            ..ConnectionSettings::default()
        };
        for (name, value) in properties {
            settings
                .properties
                .insert((*name).to_owned(), (*value).to_owned());
        }
        ConnectionConfig::new(settings, None)
    }

    #[test]
    fn account_property_selects_the_account_url() {
        let settings = SnowflakeSettings::from_config(&config(&[
            ("account", "myorg-acct"),
            ("warehouse", "compute_wh"),
            ("role", "\"Analyst\""),
        ]))
        .unwrap();
        assert_eq!(
            settings.base_url.as_str(),
            "https://myorg-acct.snowflakecomputing.com/"
        );
        assert_eq!(settings.auth, AuthMethod::KeyPair);
        assert_eq!(settings.context.database.as_deref(), Some("SALES"));
        assert_eq!(settings.context.warehouse.as_deref(), Some("COMPUTE_WH"));
        assert_eq!(settings.context.role.as_deref(), Some("Analyst"));
    }

    #[test]
    fn tunnels_keep_the_original_account_host() {
        let mut original = config(&[]);
        original.settings.host = Some("acme.snowflakecomputing.com".into());
        let tunneled = original.redirected("127.0.0.1", 40001);
        let settings = SnowflakeSettings::from_config(&tunneled).unwrap();
        assert_eq!(
            settings.base_url.as_str(),
            "https://acme.snowflakecomputing.com:40001/"
        );
        assert_eq!(settings.account.jwt_name(), "ACME");
        let unnamed = config(&[("account", "acme")]).redirected("127.0.0.1", 40001);
        assert!(SnowflakeSettings::from_config(&unnamed).is_err());
    }

    #[test]
    fn password_logins_are_explained() {
        let error = SnowflakeSettings::from_config(&config(&[
            ("account", "a"),
            ("authenticator", "snowflake"),
        ]))
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsupported);
        assert!(error.message.contains("does not accept password"));
    }
}
