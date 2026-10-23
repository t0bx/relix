/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::account::Account;
use crate::auth::private_key::public_key_fingerprint;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use relix_core::Secret;
use relix_driver_api::DriverError;
use rsa::RsaPrivateKey;
use rsa::pkcs1::EncodeRsaPrivateKey;
use serde::Serialize;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const TOKEN_LIFETIME_SECS: u64 = 59 * 60;
const RENEWAL_MARGIN_SECS: u64 = 5 * 60;

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    sub: &'a str,
    iat: u64,
    exp: u64,
}

struct IssuedToken {
    token: Secret,
    expires_at: u64,
}

/// Issues the RS256-signed JWTs of Snowflake key-pair authentication
/// (`iss = ACCOUNT.USER.SHA256:<fingerprint>`, `sub = ACCOUNT.USER`) and reuses each
/// token until shortly before it expires. Tokens only live in memory.
pub(crate) struct KeyPairToken {
    key: EncodingKey,
    issuer: String,
    subject: String,
    issued: Mutex<Option<IssuedToken>>,
}

impl KeyPairToken {
    pub(crate) fn new(
        key: &RsaPrivateKey,
        account: &Account,
        user: &str,
    ) -> Result<Self, DriverError> {
        let subject = format!("{}.{}", account.jwt_name(), user.to_uppercase());
        let issuer = format!("{subject}.{}", public_key_fingerprint(key)?);
        let der = key.to_pkcs1_der().map_err(|error| {
            DriverError::internal(format!("cannot encode private key: {error}"))
        })?;
        Ok(Self {
            key: EncodingKey::from_rsa_der(der.as_bytes()),
            issuer,
            subject,
            issued: Mutex::new(None),
        })
    }

    pub(crate) fn token(&self) -> Result<Secret, DriverError> {
        let now = unix_now();
        let mut issued = self
            .issued
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(current) = issued.as_ref()
            && current.expires_at > now + RENEWAL_MARGIN_SECS
        {
            return Ok(current.token.clone());
        }
        let claims = Claims {
            iss: &self.issuer,
            sub: &self.subject,
            iat: now,
            exp: now + TOKEN_LIFETIME_SECS,
        };
        let token = jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &self.key)
            .map_err(|error| DriverError::internal(format!("cannot sign JWT: {error}")))?;
        let token = Secret::new(token);
        *issued = Some(IssuedToken {
            token: token.clone(),
            expires_at: claims.exp,
        });
        Ok(token)
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}
