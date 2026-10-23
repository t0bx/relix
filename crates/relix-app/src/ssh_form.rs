/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::datasource_form::ServerChoice;
use crate::form::{FieldValue, FormCommand, FormField, FormRow, FormState, FormStatus, FormView};
use relix_config::KeyChord;
use relix_core::{
    ConfigScope, HostKeyPolicy, Secret, ServerConfigId, SshAuthMethod, SshServerConfig,
};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshField {
    Name,
    Scope,
    Host,
    Port,
    User,
    Auth,
    PrivateKey,
    Secret,
    SaveSecret,
    UseSshConfig,
    KnownHosts,
    HostKeyPolicy,
    ConnectTimeout,
    Keepalive,
    JumpHost,
    BindAddress,
    Reconnect,
    ReconnectAttempts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshForm {
    pub editing: Option<ServerConfigId>,
    pub form: FormState<SshField>,
    scope: ConfigScope,
    scopes: Vec<ConfigScope>,
    jump_ids: Vec<Option<ServerConfigId>>,
    original: Option<SshServerConfig>,
}

impl SshForm {
    pub fn new(
        existing: Option<(ConfigScope, &SshServerConfig)>,
        jump_choices: &[ServerChoice],
        project_open: bool,
    ) -> Self {
        let config = existing.map_or_else(
            || SshServerConfig::new("", ""),
            |(_, config)| config.clone(),
        );
        let scope = existing.map_or(ConfigScope::Global, |(scope, _)| scope);
        let scopes = if project_open {
            vec![ConfigScope::Global, ConfigScope::Project]
        } else {
            vec![ConfigScope::Global]
        };
        let mut jump_labels = vec!["(none)".to_owned()];
        let mut jump_ids = vec![None];
        for choice in jump_choices.iter().filter(|choice| choice.id != config.id) {
            jump_labels.push(choice.label.clone());
            jump_ids.push(Some(choice.id));
        }
        let jump_index = jump_ids
            .iter()
            .position(|id| *id == config.jump_host)
            .unwrap_or(0);
        let auth_index = SshAuthMethod::ALL
            .iter()
            .position(|method| *method == config.auth)
            .unwrap_or(0);
        let policy_index = HostKeyPolicy::ALL
            .iter()
            .position(|policy| *policy == config.host_key_policy)
            .unwrap_or(0);
        let path_text = |path: &Option<PathBuf>| {
            path.as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default()
        };
        let fields = vec![
            FormField::text(SshField::Name, "Name", &config.name),
            FormField::choice(
                SshField::Scope,
                "Scope",
                scopes
                    .iter()
                    .map(|scope| scope.label().to_owned())
                    .collect(),
                scopes.iter().position(|item| *item == scope).unwrap_or(0),
            ),
            FormField::text(SshField::Host, "SSH host", &config.host),
            FormField::text(SshField::Port, "SSH port", &config.port.to_string()),
            FormField::text(SshField::User, "SSH user", &config.username),
            FormField::choice(
                SshField::Auth,
                "Auth method",
                SshAuthMethod::ALL
                    .iter()
                    .map(|method| method.label().to_owned())
                    .collect(),
                auth_index,
            ),
            FormField::text(
                SshField::PrivateKey,
                "Private key",
                &path_text(&config.private_key),
            ),
            FormField::secret(SshField::Secret, "Secret"),
            FormField::toggle(SshField::SaveSecret, "Save secret", config.save_secret),
            FormField::toggle(
                SshField::UseSshConfig,
                "Use ~/.ssh/config",
                config.use_ssh_config,
            ),
            FormField::text(
                SshField::KnownHosts,
                "Known hosts",
                &path_text(&config.known_hosts_file),
            ),
            FormField::choice(
                SshField::HostKeyPolicy,
                "Host keys",
                HostKeyPolicy::ALL
                    .iter()
                    .map(|policy| policy.label().to_owned())
                    .collect(),
                policy_index,
            ),
            FormField::text(
                SshField::ConnectTimeout,
                "Timeout (s)",
                &config.connect_timeout_secs.to_string(),
            ),
            FormField::text(
                SshField::Keepalive,
                "Keepalive (s)",
                &config.keepalive_interval_secs.to_string(),
            ),
            FormField::choice(SshField::JumpHost, "Jump host", jump_labels, jump_index),
            FormField::text(
                SshField::BindAddress,
                "Bind address",
                &config.local_bind_address,
            ),
            FormField::toggle(SshField::Reconnect, "Reconnect", config.reconnect.enabled),
            FormField::text(
                SshField::ReconnectAttempts,
                "Attempts",
                &config.reconnect.max_attempts.to_string(),
            ),
        ];
        Self {
            editing: existing.map(|(_, config)| config.id),
            form: FormState::new(fields),
            scope,
            scopes,
            jump_ids,
            original: existing.map(|(_, config)| config.clone()),
        }
    }

    fn auth(&self) -> SshAuthMethod {
        SshAuthMethod::ALL[self
            .form
            .choice(SshField::Auth)
            .min(SshAuthMethod::ALL.len() - 1)]
    }

    pub fn is_visible(&self, id: SshField) -> bool {
        match id {
            SshField::Scope => self.editing.is_none(),
            SshField::PrivateKey => self.auth() == SshAuthMethod::PrivateKey,
            SshField::Secret | SshField::SaveSecret => self.auth().uses_secret(),
            SshField::ReconnectAttempts => self.form.toggle(SshField::Reconnect),
            _ => true,
        }
    }

    pub fn handle_key(&mut self, chord: KeyChord) -> FormCommand<SshField> {
        let visible: Vec<SshField> = self
            .form
            .fields
            .iter()
            .map(|field| field.id)
            .filter(|id| self.is_visible(*id))
            .collect();
        self.form.handle_key(chord, |id| visible.contains(&id))
    }

    pub fn scope(&self) -> ConfigScope {
        if self.editing.is_some() {
            self.scope
        } else {
            self.scopes
                .get(self.form.choice(SshField::Scope))
                .copied()
                .unwrap_or(ConfigScope::Global)
        }
    }

    pub fn set_status(&mut self, status: FormStatus, busy: bool) {
        self.form.status = Some(status);
        self.form.busy = busy;
    }

    fn number<T: std::str::FromStr>(&self, id: SshField, what: &str) -> Result<T, String> {
        let text = self.form.text(id);
        text.parse::<T>()
            .map_err(|_| format!("{what} must be a number, not '{text}'"))
    }

    /// Validates the form. Returns the configuration, its scope and the secret typed in
    /// this dialog; `None` for the secret means "keep whatever is stored".
    pub fn build(&self) -> Result<(ConfigScope, SshServerConfig, Option<Secret>), String> {
        let name = self.form.text(SshField::Name);
        if name.is_empty() {
            return Err("name is required".into());
        }
        let host = self.form.text(SshField::Host);
        if host.is_empty() {
            return Err("SSH host is required".into());
        }
        let use_ssh_config = self.form.toggle(SshField::UseSshConfig);
        let username = self.form.text(SshField::User);
        if username.is_empty() && !use_ssh_config {
            return Err("SSH user is required".into());
        }
        let auth = self.auth();
        let private_key = self.form.optional(SshField::PrivateKey).map(PathBuf::from);
        if auth == SshAuthMethod::PrivateKey && private_key.is_none() && !use_ssh_config {
            return Err("select a private key file".into());
        }
        let port: u16 = self.number(SshField::Port, "SSH port")?;
        let mut config = self
            .original
            .clone()
            .unwrap_or_else(|| SshServerConfig::new(name.clone(), host.clone()));
        config.name = name;
        config.host = host;
        config.port = port;
        config.username = username;
        config.auth = auth;
        config.private_key = private_key.filter(|_| auth == SshAuthMethod::PrivateKey);
        config.save_secret = auth.uses_secret() && self.form.toggle(SshField::SaveSecret);
        config.use_ssh_config = use_ssh_config;
        config.known_hosts_file = self.form.optional(SshField::KnownHosts).map(PathBuf::from);
        config.host_key_policy = HostKeyPolicy::ALL[self
            .form
            .choice(SshField::HostKeyPolicy)
            .min(HostKeyPolicy::ALL.len() - 1)];
        config.connect_timeout_secs = self
            .number::<u64>(SshField::ConnectTimeout, "timeout")?
            .clamp(1, 600);
        config.keepalive_interval_secs = self.number::<u64>(SshField::Keepalive, "keepalive")?;
        config.jump_host = self
            .jump_ids
            .get(self.form.choice(SshField::JumpHost))
            .copied()
            .flatten();
        let bind = self.form.text(SshField::BindAddress);
        if bind.parse::<std::net::IpAddr>().is_err() {
            return Err(format!("bind address must be an IP address, not '{bind}'"));
        }
        config.local_bind_address = bind;
        config.reconnect.enabled = self.form.toggle(SshField::Reconnect);
        config.reconnect.max_attempts = self
            .number::<u32>(SshField::ReconnectAttempts, "reconnect attempts")?
            .clamp(1, 20);
        let secret = (auth.uses_secret() && self.form.is_touched(SshField::Secret))
            .then(|| self.form.text(SshField::Secret))
            .filter(|secret| !secret.is_empty())
            .map(Secret::new);
        Ok((self.scope(), config, secret))
    }
}

impl FormView for SshForm {
    fn title(&self) -> String {
        if self.editing.is_some() {
            "Edit SSH configuration".into()
        } else {
            "New SSH configuration".into()
        }
    }

    fn rows(&self) -> Vec<FormRow<'_>> {
        let stored = self
            .original
            .as_ref()
            .is_some_and(|config| config.save_secret);
        self.form
            .fields
            .iter()
            .enumerate()
            .filter(|(_, field)| self.is_visible(field.id))
            .map(|(index, field)| FormRow {
                label: match field.id {
                    SshField::Secret if self.auth() == SshAuthMethod::Password => "Password",
                    SshField::Secret => "Key passphrase",
                    _ => field.label,
                },
                value: &field.value,
                focused: index == self.form.focused,
                placeholder: match (&field.value, field.id) {
                    (FieldValue::Secret(input), SshField::Secret)
                        if input.is_empty() && !self.form.is_touched(SshField::Secret) =>
                    {
                        Some(String::from(if stored {
                            "(stored)"
                        } else {
                            "(asked when needed)"
                        }))
                    }
                    (FieldValue::Text(input), SshField::KnownHosts) if input.is_empty() => {
                        Some("~/.ssh/known_hosts + Relix known hosts".into())
                    }
                    (FieldValue::Text(input), SshField::PrivateKey) if input.is_empty() => {
                        Some("~/.ssh/id_ed25519".into())
                    }
                    _ => None,
                },
            })
            .collect()
    }

    fn status(&self) -> Option<&FormStatus> {
        self.form.status.as_ref()
    }

    fn busy(&self) -> bool {
        self.form.busy
    }

    fn footer(&self) -> &'static str {
        "Tab/↑↓ field · ←→ change · Space toggle · Ctrl+T test · Enter save · Esc cancel"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_config::{KeyCode, Modifiers};

    fn filled() -> SshForm {
        let mut form = SshForm::new(None, &[], true);
        form.form.set_text(SshField::Name, "Production Bastion");
        form.form.set_text(SshField::Host, "bastion.example.com");
        form.form.set_text(SshField::User, "deploy");
        form.form.set_text(SshField::PrivateKey, "~/.ssh/deploy");
        form
    }

    #[test]
    fn builds_a_key_based_configuration() {
        let (scope, config, secret) = filled().build().unwrap();
        assert_eq!(scope, ConfigScope::Global);
        assert_eq!(config.auth, SshAuthMethod::PrivateKey);
        assert_eq!(config.private_key, Some(PathBuf::from("~/.ssh/deploy")));
        assert_eq!(config.port, 22);
        assert_eq!(secret, None);
    }

    #[test]
    fn auth_method_controls_visible_fields() {
        let mut form = filled();
        form.form.focused = form
            .form
            .fields
            .iter()
            .position(|field| field.id == SshField::Auth)
            .unwrap();
        form.handle_key(KeyChord::new(KeyCode::Right, Modifiers::NONE));
        assert!(!form.is_visible(SshField::PrivateKey));
        assert!(form.is_visible(SshField::Secret));
        form.handle_key(KeyChord::new(KeyCode::Right, Modifiers::NONE));
        assert!(!form.is_visible(SshField::Secret));
        let (_, config, _) = form.build().unwrap();
        assert_eq!(config.auth, SshAuthMethod::Agent);
        assert_eq!(config.private_key, None);
        assert!(!config.save_secret);
    }

    #[test]
    fn editing_keeps_identity_and_jump_host() {
        let gateway = ServerChoice {
            id: ServerConfigId::new(),
            label: "Gateway · global".into(),
        };
        let mut existing = SshServerConfig::new("Bastion", "bastion");
        existing.username = "deploy".into();
        existing.auth = SshAuthMethod::Agent;
        existing.jump_host = Some(gateway.id);
        let form = SshForm::new(Some((ConfigScope::Project, &existing)), &[gateway], true);
        assert!(!form.is_visible(SshField::Scope));
        let (scope, config, _) = form.build().unwrap();
        assert_eq!(scope, ConfigScope::Project);
        assert_eq!(config.id, existing.id);
        assert_eq!(config.jump_host, existing.jump_host);
    }

    #[test]
    fn invalid_values_are_reported() {
        let mut form = filled();
        form.form.set_text(SshField::Port, "abc");
        assert!(form.build().unwrap_err().contains("SSH port"));
        form.form.set_text(SshField::Port, "22");
        form.form.set_text(SshField::BindAddress, "localhost");
        assert!(form.build().unwrap_err().contains("bind address"));
    }
}
