/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::{CredentialKind, SshError};
use crate::handler::ClientHandler;
use crate::hop::{HopSecrets, ResolvedHop};
use relix_core::SshAuthMethod;
use russh::client::Handle;
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::{PrivateKeyWithHashAlg, load_secret_key};
use std::path::Path;
use std::sync::Arc;

pub(crate) async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    hop: &ResolvedHop,
    secrets: &HopSecrets,
) -> Result<(), SshError> {
    let accepted = match hop.auth {
        SshAuthMethod::Password => password(handle, hop, secrets).await?,
        SshAuthMethod::PrivateKey => private_key(handle, hop, secrets).await?,
        SshAuthMethod::Agent => agent(handle, hop).await?,
    };
    if accepted {
        Ok(())
    } else {
        Err(SshError::Authentication {
            endpoint: hop.endpoint(),
            user: hop.user.clone(),
        })
    }
}

async fn password(
    handle: &mut Handle<ClientHandler>,
    hop: &ResolvedHop,
    secrets: &HopSecrets,
) -> Result<bool, SshError> {
    let password = secrets
        .password
        .as_ref()
        .ok_or_else(|| SshError::CredentialsRequired {
            server: hop.name.clone(),
            kind: CredentialKind::Password,
        })?;
    handle
        .authenticate_password(&hop.user, password.expose())
        .await
        .map(|result| result.success())
        .map_err(|error| SshError::connection(&hop.endpoint(), error))
}

async fn private_key(
    handle: &mut Handle<ClientHandler>,
    hop: &ResolvedHop,
    secrets: &HopSecrets,
) -> Result<bool, SshError> {
    let path = hop
        .private_key
        .as_deref()
        .ok_or_else(|| SshError::Configuration(format!("'{}' has no private key", hop.name)))?;
    let key = load_key(path, hop, secrets)?;
    let hash = handle
        .best_supported_rsa_hash()
        .await
        .map_err(|error| SshError::connection(&hop.endpoint(), error))?
        .flatten();
    handle
        .authenticate_publickey(&hop.user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
        .await
        .map(|result| result.success())
        .map_err(|error| SshError::connection(&hop.endpoint(), error))
}

fn load_key(
    path: &Path,
    hop: &ResolvedHop,
    secrets: &HopSecrets,
) -> Result<russh::keys::PrivateKey, SshError> {
    let passphrase = secrets.passphrase.as_ref().map(|secret| secret.expose());
    load_secret_key(path, passphrase).map_err(|error| match error {
        russh::keys::Error::KeyIsEncrypted => SshError::CredentialsRequired {
            server: hop.name.clone(),
            kind: CredentialKind::Passphrase,
        },
        other if passphrase.is_some() && is_decryption_failure(&other) => SshError::Key {
            path: path.to_path_buf(),
            message: "wrong passphrase or unreadable key".into(),
        },
        other => SshError::Key {
            path: path.to_path_buf(),
            message: other.to_string(),
        },
    })
}

fn is_decryption_failure(error: &russh::keys::Error) -> bool {
    !matches!(error, russh::keys::Error::IO(_))
}

async fn agent(handle: &mut Handle<ClientHandler>, hop: &ResolvedHop) -> Result<bool, SshError> {
    #[cfg(unix)]
    {
        let client = AgentClient::connect_env()
            .await
            .map_err(|error| SshError::Agent(agent_hint(&error)))?;
        try_agent_identities(handle, hop, client).await
    }
    #[cfg(windows)]
    {
        const OPENSSH_AGENT_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";
        match AgentClient::connect_named_pipe(OPENSSH_AGENT_PIPE).await {
            Ok(client) => try_agent_identities(handle, hop, client).await,
            Err(_) => {
                let client = AgentClient::connect_pageant()
                    .await
                    .map_err(|error| SshError::Agent(error.to_string()))?;
                try_agent_identities(handle, hop, client).await
            }
        }
    }
}

#[cfg(unix)]
fn agent_hint(error: &russh::keys::Error) -> String {
    match error {
        russh::keys::Error::EnvVar(_) => "SSH_AUTH_SOCK is not set; start ssh-agent first".into(),
        other => other.to_string(),
    }
}

async fn try_agent_identities<S>(
    handle: &mut Handle<ClientHandler>,
    hop: &ResolvedHop,
    mut client: AgentClient<S>,
) -> Result<bool, SshError>
where
    S: AgentStream + Unpin + Send + 'static,
{
    let identities = client
        .request_identities()
        .await
        .map_err(|error| SshError::Agent(error.to_string()))?;
    if identities.is_empty() {
        return Err(SshError::Agent("the agent holds no keys".into()));
    }
    let hash = handle
        .best_supported_rsa_hash()
        .await
        .map_err(|error| SshError::connection(&hop.endpoint(), error))?
        .flatten();
    for identity in identities {
        let AgentIdentity::PublicKey { key, .. } = identity else {
            continue;
        };
        let result = handle
            .authenticate_publickey_with(&hop.user, key, hash, &mut client)
            .await
            .map_err(|error| SshError::Agent(error.to_string()))?;
        if result.success() {
            return Ok(true);
        }
    }
    Ok(false)
}
