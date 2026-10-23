/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::form::{FieldValue, FormCommand, FormField, FormRow, FormState, FormStatus, FormView};
use relix_config::KeyChord;
use relix_core::{ConfigScope, ServerConfigId, VpnMode, VpnProfile, VpnProvider};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpnField {
    Name,
    Scope,
    Mode,
    Provider,
    ConfigPath,
    Interface,
    ProbeAddress,
    AutoConnect,
    AutoDisconnect,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpnForm {
    pub editing: Option<ServerConfigId>,
    pub form: FormState<VpnField>,
    pub connection_status: String,
    scope: ConfigScope,
    scopes: Vec<ConfigScope>,
    original: Option<VpnProfile>,
}

impl VpnForm {
    pub fn new(
        existing: Option<(ConfigScope, &VpnProfile)>,
        project_open: bool,
        connection_status: String,
    ) -> Self {
        let profile = existing.map_or_else(|| VpnProfile::new(""), |(_, profile)| profile.clone());
        let scope = existing.map_or(ConfigScope::Global, |(scope, _)| scope);
        let scopes = if project_open {
            vec![ConfigScope::Global, ConfigScope::Project]
        } else {
            vec![ConfigScope::Global]
        };
        let fields = vec![
            FormField::text(VpnField::Name, "Name", &profile.name),
            FormField::choice(
                VpnField::Scope,
                "Scope",
                scopes
                    .iter()
                    .map(|scope| scope.label().to_owned())
                    .collect(),
                scopes.iter().position(|item| *item == scope).unwrap_or(0),
            ),
            FormField::choice(
                VpnField::Mode,
                "Connection",
                VpnMode::ALL
                    .iter()
                    .map(|mode| mode.label().to_owned())
                    .collect(),
                VpnMode::ALL
                    .iter()
                    .position(|mode| *mode == profile.mode)
                    .unwrap_or(0),
            ),
            FormField::choice(
                VpnField::Provider,
                "Provider",
                VpnProvider::ALL
                    .iter()
                    .map(|provider| provider.label().to_owned())
                    .collect(),
                VpnProvider::ALL
                    .iter()
                    .position(|provider| *provider == profile.provider)
                    .unwrap_or(0),
            ),
            FormField::text(
                VpnField::ConfigPath,
                "Config file",
                &profile
                    .config_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ),
            FormField::text(
                VpnField::Interface,
                "Interface",
                profile.interface.as_deref().unwrap_or_default(),
            ),
            FormField::text(
                VpnField::ProbeAddress,
                "Check address",
                profile.probe_address.as_deref().unwrap_or_default(),
            ),
            FormField::toggle(VpnField::AutoConnect, "Auto connect", profile.auto_connect),
            FormField::toggle(
                VpnField::AutoDisconnect,
                "Auto disconnect",
                profile.auto_disconnect,
            ),
            FormField::text(
                VpnField::Timeout,
                "Timeout (s)",
                &profile.connect_timeout_secs.to_string(),
            ),
        ];
        Self {
            editing: existing.map(|(_, profile)| profile.id),
            form: FormState::new(fields),
            connection_status,
            scope,
            scopes,
            original: existing.map(|(_, profile)| profile.clone()),
        }
    }

    fn mode(&self) -> VpnMode {
        VpnMode::ALL[self.form.choice(VpnField::Mode).min(VpnMode::ALL.len() - 1)]
    }

    fn provider(&self) -> VpnProvider {
        VpnProvider::ALL[self
            .form
            .choice(VpnField::Provider)
            .min(VpnProvider::ALL.len() - 1)]
    }

    pub fn is_visible(&self, id: VpnField) -> bool {
        let managed = self.mode() == VpnMode::Managed;
        match id {
            VpnField::Scope => self.editing.is_none(),
            VpnField::ConfigPath
            | VpnField::AutoConnect
            | VpnField::AutoDisconnect
            | VpnField::Timeout => managed,
            _ => true,
        }
    }

    pub fn handle_key(&mut self, chord: KeyChord) -> FormCommand<VpnField> {
        let visible: Vec<VpnField> = self
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
                .get(self.form.choice(VpnField::Scope))
                .copied()
                .unwrap_or(ConfigScope::Global)
        }
    }

    pub fn set_status(&mut self, status: FormStatus, busy: bool) {
        self.form.status = Some(status);
        self.form.busy = busy;
    }

    pub fn build(&self) -> Result<(ConfigScope, VpnProfile), String> {
        let name = self.form.text(VpnField::Name);
        if name.is_empty() {
            return Err("name is required".into());
        }
        let mode = self.mode();
        let provider = self.provider();
        let config_path = self.form.optional(VpnField::ConfigPath).map(PathBuf::from);
        if mode == VpnMode::Managed {
            if !provider.is_manageable() {
                return Err("Relix can only manage WireGuard and OpenVPN profiles".into());
            }
            if config_path.is_none() {
                return Err("select the VPN configuration file".into());
            }
        }
        let timeout = self
            .form
            .text(VpnField::Timeout)
            .parse::<u64>()
            .map_err(|_| "timeout must be a number of seconds".to_owned())?;
        let mut profile = self
            .original
            .clone()
            .unwrap_or_else(|| VpnProfile::new(name.clone()));
        profile.name = name;
        profile.mode = mode;
        profile.provider = provider;
        profile.config_path = config_path.filter(|_| mode == VpnMode::Managed);
        profile.interface = self.form.optional(VpnField::Interface);
        profile.probe_address = self.form.optional(VpnField::ProbeAddress);
        profile.auto_connect = mode == VpnMode::Managed && self.form.toggle(VpnField::AutoConnect);
        profile.auto_disconnect = self.form.toggle(VpnField::AutoDisconnect);
        profile.connect_timeout_secs = timeout.clamp(1, 600);
        Ok((self.scope(), profile))
    }
}

impl FormView for VpnForm {
    fn title(&self) -> String {
        let action = if self.editing.is_some() {
            "Edit"
        } else {
            "New"
        };
        format!("{action} VPN profile — {}", self.connection_status)
    }

    fn rows(&self) -> Vec<FormRow<'_>> {
        self.form
            .fields
            .iter()
            .enumerate()
            .filter(|(_, field)| self.is_visible(field.id))
            .map(|(index, field)| FormRow {
                label: field.label,
                value: &field.value,
                focused: index == self.form.focused,
                placeholder: match (&field.value, field.id) {
                    (FieldValue::Text(input), VpnField::Interface) if input.is_empty() => {
                        Some("e.g. wg0, utun4 (verifies the tunnel)".into())
                    }
                    (FieldValue::Text(input), VpnField::ProbeAddress) if input.is_empty() => {
                        Some("host:port reachable only through the VPN".into())
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
        "Ctrl+T test · Ctrl+O connect · Ctrl+X disconnect · Enter save · Esc cancel"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_config::{KeyCode, Modifiers};

    #[test]
    fn existing_vpns_need_no_configuration_file() {
        let mut form = VpnForm::new(None, true, "disconnected".into());
        form.form.set_text(VpnField::Name, "Company VPN");
        form.form.set_text(VpnField::Interface, "utun4");
        assert!(!form.is_visible(VpnField::ConfigPath));
        let (_, profile) = form.build().unwrap();
        assert_eq!(profile.mode, VpnMode::Existing);
        assert_eq!(profile.interface.as_deref(), Some("utun4"));
        assert!(!profile.auto_connect);
    }

    #[test]
    fn managed_profiles_require_a_supported_provider_and_file() {
        let mut form = VpnForm::new(None, false, "disconnected".into());
        form.form.set_text(VpnField::Name, "Production WireGuard");
        form.form.focused = form
            .form
            .fields
            .iter()
            .position(|field| field.id == VpnField::Mode)
            .unwrap();
        form.handle_key(KeyChord::new(KeyCode::Right, Modifiers::NONE));
        assert!(form.is_visible(VpnField::ConfigPath));
        assert_eq!(
            form.build().unwrap_err(),
            "select the VPN configuration file"
        );
        form.form
            .set_text(VpnField::ConfigPath, "~/.config/relix/vpn/production.conf");
        let (scope, profile) = form.build().unwrap();
        assert_eq!(scope, ConfigScope::Global);
        assert_eq!(profile.mode, VpnMode::Managed);
        assert_eq!(profile.provider, VpnProvider::WireGuard);
        form.form.set_choice(VpnField::Provider, 2);
        assert!(form.build().unwrap_err().contains("only manage"));
    }
}
