/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::driver::DEFAULT_PORT;
use crate::tls::tls_options;
use mongodb::options::{AuthMechanism, ClientOptions, Credential, ServerAddress};
use relix_core::ConnectionConfig;
use relix_driver_api::DriverError;
use std::str::FromStr;

pub const URI_PROPERTY: &str = "uri";
pub const AUTH_SOURCE_PROPERTY: &str = "authSource";
pub const AUTH_MECHANISM_PROPERTY: &str = "authMechanism";
pub const REPLICA_SET_PROPERTY: &str = "replicaSet";
pub const DIRECT_CONNECTION_PROPERTY: &str = "directConnection";
const DEFAULT_AUTH_SOURCE: &str = "admin";
const APP_NAME: &str = "relix";

/// Builds the client options either from the `uri` property (SRV records, replica sets,
/// any connection string option) or from the host, port, username and password fields.
/// With a URI, the username/password fields only fill in credentials the URI leaves out,
/// so the password can stay in the keyring; TLS settings apply when the URI does not
/// configure TLS itself.
pub async fn client_options(config: &ConnectionConfig) -> Result<ClientOptions, DriverError> {
    let mut options = match config.settings.property(URI_PROPERTY) {
        Some(uri) => options_from_uri(config, uri).await?,
        None => options_from_fields(config)?,
    };
    let timeout = config.settings.connect_timeout();
    options.app_name = Some(APP_NAME.to_owned());
    options.connect_timeout = Some(timeout);
    options.server_selection_timeout = Some(timeout);
    if options.default_database.is_none() {
        options.default_database = configured_database(config).map(str::to_owned);
    }
    Ok(options)
}

pub fn configured_database(config: &ConnectionConfig) -> Option<&str> {
    config
        .settings
        .database
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

async fn options_from_uri(
    config: &ConnectionConfig,
    uri: &str,
) -> Result<ClientOptions, DriverError> {
    if config.tls_server_name.is_some() {
        return Err(DriverError::configuration(
            "a MongoDB connection string cannot be routed through an SSH tunnel: clear the \
             'uri' property and use the host and port fields instead",
        ));
    }
    let mut options = ClientOptions::parse(uri).await.map_err(|error| {
        DriverError::configuration(redact(
            &format!("invalid MongoDB connection string: {error}"),
            uri,
        ))
    })?;
    match options.credential.as_mut() {
        Some(credential) if credential.password.is_none() => {
            credential.password = config
                .password
                .as_ref()
                .map(|secret| secret.expose().to_owned());
        }
        Some(_) => {}
        None => options.credential = field_credential(config)?,
    }
    if options.tls.is_none() {
        options.tls = Some(tls_options(config)?);
    }
    Ok(options)
}

fn options_from_fields(config: &ConnectionConfig) -> Result<ClientOptions, DriverError> {
    let host = ServerAddress::Tcp {
        host: config.host_or_default().to_owned(),
        port: Some(config.settings.port.unwrap_or(DEFAULT_PORT)),
    };
    let direct_connection = match config.settings.property(DIRECT_CONNECTION_PROPERTY) {
        Some(value) => Some(parse_flag(DIRECT_CONNECTION_PROPERTY, value)?),
        None => config.tls_server_name.as_ref().map(|_| true),
    };
    Ok(ClientOptions::builder()
        .hosts(vec![host])
        .credential(field_credential(config)?)
        .tls(tls_options(config)?)
        .direct_connection(direct_connection)
        .repl_set_name(
            config
                .settings
                .property(REPLICA_SET_PROPERTY)
                .map(str::to_owned),
        )
        .build())
}

fn field_credential(config: &ConnectionConfig) -> Result<Option<Credential>, DriverError> {
    let settings = &config.settings;
    let username = settings
        .username
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let mechanism = settings
        .property(AUTH_MECHANISM_PROPERTY)
        .map(parse_mechanism)
        .transpose()?;
    if username.is_none() && mechanism.is_none() {
        return Ok(None);
    }
    let source = settings
        .property(AUTH_SOURCE_PROPERTY)
        .unwrap_or(DEFAULT_AUTH_SOURCE);
    Ok(Some(
        Credential::builder()
            .username(username.map(str::to_owned))
            .password(
                config
                    .password
                    .as_ref()
                    .map(|secret| secret.expose().to_owned()),
            )
            .source(source.to_owned())
            .mechanism(mechanism)
            .build(),
    ))
}

fn parse_mechanism(value: &str) -> Result<AuthMechanism, DriverError> {
    AuthMechanism::from_str(value).map_err(|_| {
        DriverError::configuration(format!(
            "unsupported authMechanism \"{value}\": use SCRAM-SHA-256, SCRAM-SHA-1, PLAIN, \
             MONGODB-X509 or MONGODB-OIDC"
        ))
    })
}

fn parse_flag(name: &str, value: &str) -> Result<bool, DriverError> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(DriverError::configuration(format!(
            "property {name} must be true or false, got \"{value}\""
        ))),
    }
}

fn redact(message: &str, uri: &str) -> String {
    let user_info = uri
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('@'))
        .map(|(user_info, _)| user_info)
        .filter(|user_info| !user_info.is_empty());
    match user_info {
        Some(user_info) => message.replace(user_info, "***"),
        None => message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::{ConnectionSettings, Secret};

    fn config(properties: &[(&str, &str)]) -> ConnectionConfig {
        let mut settings = ConnectionSettings {
            host: Some("mongo.internal".into()),
            port: Some(27018),
            username: Some("app".into()),
            database: Some("shop".into()),
            ..ConnectionSettings::default()
        };
        for (name, value) in properties {
            settings
                .properties
                .insert((*name).to_owned(), (*value).to_owned());
        }
        ConnectionConfig::new(settings, Some(Secret::new("s3cret")))
    }

    #[tokio::test]
    async fn fields_build_a_single_host_with_admin_credentials() {
        let options = client_options(&config(&[])).await.unwrap();
        assert_eq!(options.hosts[0].to_string(), "mongo.internal:27018");
        let credential = options.credential.unwrap();
        assert_eq!(credential.username.as_deref(), Some("app"));
        assert_eq!(credential.password.as_deref(), Some("s3cret"));
        assert_eq!(credential.source.as_deref(), Some("admin"));
        assert_eq!(options.default_database.as_deref(), Some("shop"));
        assert_eq!(options.direct_connection, None);
    }

    #[tokio::test]
    async fn properties_select_auth_source_mechanism_and_topology() {
        let options = client_options(&config(&[
            ("authSource", "shop"),
            ("authMechanism", "SCRAM-SHA-256"),
            ("replicaSet", "rs0"),
            ("directConnection", "false"),
        ]))
        .await
        .unwrap();
        let credential = options.credential.unwrap();
        assert_eq!(credential.source.as_deref(), Some("shop"));
        assert_eq!(credential.mechanism, Some(AuthMechanism::ScramSha256));
        assert_eq!(options.repl_set_name.as_deref(), Some("rs0"));
        assert_eq!(options.direct_connection, Some(false));
    }

    #[tokio::test]
    async fn tunnels_connect_directly_to_the_forwarded_port() {
        let tunneled = config(&[]).redirected("127.0.0.1", 41000);
        let options = client_options(&tunneled).await.unwrap();
        assert_eq!(options.hosts[0].to_string(), "127.0.0.1:41000");
        assert_eq!(options.direct_connection, Some(true));
    }

    #[tokio::test]
    async fn uri_credentials_take_the_password_from_the_keyring() {
        let options = client_options(&config(&[(
            "uri",
            "mongodb://reporter@a.example:27017,b.example:27017/sales?replicaSet=rs0",
        )]))
        .await
        .unwrap();
        assert_eq!(options.hosts.len(), 2);
        let credential = options.credential.unwrap();
        assert_eq!(credential.username.as_deref(), Some("reporter"));
        assert_eq!(credential.password.as_deref(), Some("s3cret"));
        assert_eq!(options.default_database.as_deref(), Some("sales"));
    }

    #[tokio::test]
    async fn invalid_uris_never_echo_credentials() {
        let error = client_options(&config(&[("uri", "mongodb://user:hunter2@host:notaport")]))
            .await
            .unwrap_err();
        assert!(!error.message.contains("hunter2"), "{}", error.message);
        let tunneled = config(&[("uri", "mongodb://host")]).redirected("127.0.0.1", 1);
        assert!(client_options(&tunneled).await.is_err());
    }

    #[test]
    fn unknown_mechanisms_and_flags_are_configuration_errors() {
        assert!(parse_mechanism("KERBEROS-ISH").is_err());
        assert!(parse_flag("directConnection", "maybe").is_err());
    }
}
