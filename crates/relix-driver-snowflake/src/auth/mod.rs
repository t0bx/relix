/*
 * @author t0bx
 * @since 23.10.2026
 */

mod key_pair;
pub(crate) mod private_key;

use crate::settings::{AuthMethod, SnowflakeSettings};
use key_pair::KeyPairToken;
use private_key::read_private_key;
use relix_core::Secret;
use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::RequestBuilder;

const TOKEN_TYPE_HEADER: &str = "X-Snowflake-Authorization-Token-Type";

pub(crate) enum Credentials {
    KeyPair(KeyPairToken),
    Token {
        token: Secret,
        token_type: &'static str,
    },
}

impl Credentials {
    /// Loads the credentials for the configured authenticator. The password slot holds
    /// the private key passphrase for key-pair authentication and the token otherwise.
    pub(crate) async fn load(
        settings: &SnowflakeSettings,
        password: Option<&Secret>,
    ) -> Result<Self, DriverError> {
        match settings.auth {
            AuthMethod::KeyPair => {
                let path = settings.private_key_file.as_deref().ok_or_else(|| {
                    DriverError::configuration(
                        "key-pair authentication needs the \"private_key_file\" property",
                    )
                })?;
                let user = settings
                    .user
                    .as_deref()
                    .ok_or_else(|| DriverError::configuration("set the Snowflake user name"))?;
                let key = read_private_key(path, password).await?;
                Ok(Self::KeyPair(KeyPairToken::new(
                    &key,
                    &settings.account,
                    user,
                )?))
            }
            AuthMethod::OAuth => Self::token(password, "OAUTH", "OAuth access token"),
            AuthMethod::ProgrammaticAccessToken => Self::token(
                password,
                "PROGRAMMATIC_ACCESS_TOKEN",
                "programmatic access token",
            ),
        }
    }

    fn token(
        password: Option<&Secret>,
        token_type: &'static str,
        label: &str,
    ) -> Result<Self, DriverError> {
        let token = password
            .filter(|secret| !secret.expose().trim().is_empty())
            .cloned()
            .ok_or_else(|| {
                DriverError::new(
                    ErrorKind::Authentication,
                    format!("enter the {label} as the password"),
                )
            })?;
        Ok(Self::Token { token, token_type })
    }

    pub(crate) fn authorize(&self, request: RequestBuilder) -> Result<RequestBuilder, DriverError> {
        let (token, token_type) = match self {
            Self::KeyPair(key_pair) => (key_pair.token()?, "KEYPAIR_JWT"),
            Self::Token { token, token_type } => (token.clone(), *token_type),
        };
        Ok(request
            .bearer_auth(token.expose())
            .header(TOKEN_TYPE_HEADER, token_type))
    }
}
