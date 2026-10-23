/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::form::{FieldValue, FormCommand, FormField, FormRow, FormState, FormStatus, FormView};
use crate::services::DriverDescriptor;
use relix_config::KeyChord;
use relix_core::{
    ConnectionSettings, DataSourceDefinition, DataSourceId, DriverId, Environment, NetworkMode,
    NetworkSettings, Secret, ServerConfigId, TlsMode, TlsSettings,
};
use relix_driver_api::ConnectionTarget;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
    Name,
    Driver,
    Host,
    Port,
    Username,
    Password,
    Database,
    FilePath,
    CreateIfMissing,
    TlsMode,
    CaCertificate,
    NetworkMode,
    SshConfig,
    VpnProfile,
    LocalPort,
    Timeout,
    QueryTimeout,
    Properties,
    Environment,
    Color,
    SavePassword,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerChoice {
    pub id: ServerConfigId,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerChoices {
    pub ssh: Vec<ServerChoice>,
    pub vpn: Vec<ServerChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSourceForm {
    pub editing: Option<DataSourceId>,
    pub form: FormState<FieldId>,
    pub drivers: Vec<DriverDescriptor>,
    servers: ServerChoices,
    ssh_ids: Vec<Option<ServerConfigId>>,
    vpn_ids: Vec<Option<ServerConfigId>>,
}

fn server_field(
    id: FieldId,
    label: &'static str,
    choices: &[ServerChoice],
    current: Option<ServerConfigId>,
) -> (FormField<FieldId>, Vec<Option<ServerConfigId>>) {
    let mut options: Vec<String> = choices.iter().map(|choice| choice.label.clone()).collect();
    let mut ids: Vec<Option<ServerConfigId>> =
        choices.iter().map(|choice| Some(choice.id)).collect();
    if let Some(current) = current
        && !ids.contains(&Some(current))
    {
        options.push("(deleted configuration)".into());
        ids.push(None);
    }
    if options.is_empty() {
        options.push("(none — create one in Server configurations)".into());
        ids.push(None);
    }
    let selected = current
        .and_then(|current| ids.iter().position(|id| *id == Some(current)))
        .or_else(|| {
            ids.iter()
                .position(Option::is_none)
                .filter(|_| current.is_some())
        })
        .unwrap_or(0);
    (FormField::choice(id, label, options, selected), ids)
}

fn properties_text(properties: &BTreeMap<String, String>) -> String {
    properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn parse_properties(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut properties = BTreeMap::new();
    for entry in text
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| format!("property '{entry}' must look like key=value"))?;
        let key = key.trim();
        if key.is_empty() {
            return Err(format!("property '{entry}' has no name"));
        }
        properties.insert(key.to_owned(), value.trim().to_owned());
    }
    Ok(properties)
}

impl DataSourceForm {
    pub fn new(
        drivers: Vec<DriverDescriptor>,
        existing: Option<&DataSourceDefinition>,
        servers: ServerChoices,
    ) -> Self {
        let definition = existing.cloned().unwrap_or_else(|| {
            let driver = drivers.first().map_or_else(
                || DriverId::new(DriverId::POSTGRES),
                |driver| driver.id.clone(),
            );
            DataSourceDefinition::new("", driver)
        });
        let settings = &definition.settings;
        let driver_index = drivers
            .iter()
            .position(|driver| driver.id == definition.driver)
            .unwrap_or(0);
        let index_of = |list: &[TlsMode], mode: TlsMode| {
            list.iter().position(|item| *item == mode).unwrap_or(0)
        };
        let port = settings
            .port
            .or_else(|| {
                drivers
                    .get(driver_index)
                    .and_then(|driver| driver.default_port)
            })
            .map(|port| port.to_string())
            .unwrap_or_default();
        let (ssh_field, ssh_ids) = server_field(
            FieldId::SshConfig,
            "SSH config",
            &servers.ssh,
            definition.network.ssh,
        );
        let (vpn_field, vpn_ids) = server_field(
            FieldId::VpnProfile,
            "VPN profile",
            &servers.vpn,
            definition.network.vpn,
        );
        let labels = |items: &[&'static str]| {
            items
                .iter()
                .map(|item| (*item).to_owned())
                .collect::<Vec<_>>()
        };
        let fields = vec![
            FormField::text(FieldId::Name, "Name", &definition.name),
            FormField::choice(
                FieldId::Driver,
                "Engine",
                drivers.iter().map(|driver| driver.name.clone()).collect(),
                driver_index,
            ),
            FormField::text(
                FieldId::Host,
                "Host",
                settings.host.as_deref().unwrap_or("localhost"),
            ),
            FormField::text(FieldId::Port, "Port", &port),
            FormField::text(
                FieldId::Username,
                "User",
                settings.username.as_deref().unwrap_or_default(),
            ),
            FormField::secret(FieldId::Password, "Password"),
            FormField::text(
                FieldId::Database,
                "Database",
                settings.database.as_deref().unwrap_or_default(),
            ),
            FormField::text(
                FieldId::FilePath,
                "File",
                &settings
                    .file_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ),
            FormField::toggle(
                FieldId::CreateIfMissing,
                "Create file",
                settings.create_if_missing,
            ),
            FormField::choice(
                FieldId::TlsMode,
                "TLS",
                TlsMode::ALL
                    .iter()
                    .map(|mode| mode.label().to_owned())
                    .collect(),
                index_of(&TlsMode::ALL, settings.tls.mode),
            ),
            FormField::text(
                FieldId::CaCertificate,
                "CA cert",
                &settings
                    .tls
                    .ca_certificate
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ),
            FormField::choice(
                FieldId::NetworkMode,
                "Network",
                labels(&NetworkMode::ALL.map(NetworkMode::label)),
                NetworkMode::ALL
                    .iter()
                    .position(|mode| *mode == definition.network.mode)
                    .unwrap_or(0),
            ),
            ssh_field,
            vpn_field,
            FormField::text(
                FieldId::LocalPort,
                "Local port",
                &definition
                    .network
                    .local_port
                    .map(|port| port.to_string())
                    .unwrap_or_default(),
            ),
            FormField::text(
                FieldId::Timeout,
                "Timeout (s)",
                &settings.connect_timeout_secs.to_string(),
            ),
            FormField::text(
                FieldId::QueryTimeout,
                "Query timeout",
                &settings
                    .query_timeout_secs
                    .map(|seconds| seconds.to_string())
                    .unwrap_or_default(),
            ),
            FormField::text(
                FieldId::Properties,
                "Properties",
                &properties_text(&settings.properties),
            ),
            FormField::choice(
                FieldId::Environment,
                "Environment",
                Environment::ALL
                    .iter()
                    .map(|environment| environment.label().to_owned())
                    .collect(),
                Environment::ALL
                    .iter()
                    .position(|environment| *environment == definition.environment)
                    .unwrap_or(0),
            ),
            FormField::text(
                FieldId::Color,
                "Color",
                definition.color.as_deref().unwrap_or_default(),
            ),
            FormField::toggle(
                FieldId::SavePassword,
                "Save password",
                settings.save_password,
            ),
        ];
        Self {
            editing: existing.map(|definition| definition.id),
            form: FormState::new(fields),
            drivers,
            servers,
            ssh_ids,
            vpn_ids,
        }
    }

    pub fn selected_driver(&self) -> Option<&DriverDescriptor> {
        self.drivers.get(self.form.choice(FieldId::Driver))
    }

    fn target(&self) -> ConnectionTarget {
        self.selected_driver()
            .map_or(ConnectionTarget::Network { default_port: 0 }, |driver| {
                driver.target
            })
    }

    fn network_mode(&self) -> NetworkMode {
        NetworkMode::ALL[self
            .form
            .choice(FieldId::NetworkMode)
            .min(NetworkMode::ALL.len() - 1)]
    }

    pub fn is_visible(&self, id: FieldId) -> bool {
        let target = self.target();
        let file = target == ConnectionTarget::File;
        let network = target.is_network();
        let mode = self.network_mode();
        match id {
            FieldId::Host | FieldId::Port => network,
            FieldId::Username
            | FieldId::Password
            | FieldId::TlsMode
            | FieldId::CaCertificate
            | FieldId::SavePassword
            | FieldId::NetworkMode => !file,
            FieldId::SshConfig | FieldId::LocalPort => !file && mode.uses_ssh(),
            FieldId::VpnProfile => !file && mode.uses_vpn(),
            FieldId::FilePath | FieldId::CreateIfMissing => file,
            _ => true,
        }
    }

    pub fn handle_key(&mut self, chord: KeyChord) -> FormCommand<FieldId> {
        let previous_port = self
            .selected_driver()
            .and_then(|driver| driver.default_port);
        let visible: Vec<FieldId> = self
            .form
            .fields
            .iter()
            .map(|field| field.id)
            .filter(|id| self.is_visible(*id))
            .collect();
        let command = self.form.handle_key(chord, |id| visible.contains(&id));
        if command == FormCommand::Edited(FieldId::Driver) {
            self.apply_default_port(previous_port);
        }
        command
    }

    fn apply_default_port(&mut self, previous_port: Option<u16>) {
        let current = self.form.text(FieldId::Port);
        let new_port = self
            .selected_driver()
            .and_then(|driver| driver.default_port);
        let replace =
            current.is_empty() || previous_port.map(|port| port.to_string()) == Some(current);
        if replace {
            self.form.set_text(
                FieldId::Port,
                &new_port.map(|port| port.to_string()).unwrap_or_default(),
            );
        }
    }

    pub fn set_status(&mut self, status: FormStatus, busy: bool) {
        self.form.status = Some(status);
        self.form.busy = busy;
    }

    fn selected_server(
        &self,
        field: FieldId,
        ids: &[Option<ServerConfigId>],
        kind: &str,
    ) -> Result<ServerConfigId, String> {
        ids.get(self.form.choice(field))
            .copied()
            .flatten()
            .ok_or_else(|| format!("select an existing {kind} configuration"))
    }

    fn parse_port(&self, id: FieldId) -> Result<Option<u16>, String> {
        match self.form.optional(id) {
            Some(port) => port
                .parse::<u16>()
                .map(Some)
                .map_err(|_| format!("invalid port '{port}'")),
            None => Ok(None),
        }
    }

    /// Validates the form and produces the definition plus the password to store. An
    /// untouched password field while editing means "keep the stored password".
    pub fn build(&self) -> Result<(DataSourceDefinition, Option<Secret>), String> {
        let driver = self
            .selected_driver()
            .ok_or("no database engine available")?;
        let name = self.form.text(FieldId::Name);
        if name.is_empty() {
            return Err("name is required".into());
        }
        let target = driver.target;
        let file_based = target == ConnectionTarget::File;
        let networked = target.is_network();
        let port = self.parse_port(FieldId::Port)?.or(driver.default_port);
        let timeout = self
            .form
            .text(FieldId::Timeout)
            .parse::<u64>()
            .map_err(|_| "timeout must be a number of seconds".to_owned())?;
        let query_timeout = match self.form.optional(FieldId::QueryTimeout) {
            Some(value) => Some(
                value
                    .parse::<u64>()
                    .map_err(|_| "query timeout must be a number of seconds".to_owned())?,
            ),
            None => None,
        };
        let file_path = self.form.optional(FieldId::FilePath).map(PathBuf::from);
        if file_based && file_path.is_none() {
            return Err("a database file is required".into());
        }
        let color = self.form.optional(FieldId::Color);
        if let Some(color) = &color
            && color.parse::<relix_config::ColorSpec>().is_err()
        {
            return Err(format!("invalid color '{color}'"));
        }
        let mode = if file_based {
            NetworkMode::Direct
        } else {
            self.network_mode()
        };
        if mode.uses_ssh() && !target.supports_tunnel() {
            return Err(format!(
                "{} talks to cloud APIs; SSH tunnels cannot carry it (use direct or VPN)",
                driver.name
            ));
        }
        let network = NetworkSettings {
            mode,
            ssh: if mode.uses_ssh() {
                Some(self.selected_server(FieldId::SshConfig, &self.ssh_ids, "SSH")?)
            } else {
                None
            },
            vpn: if mode.uses_vpn() {
                Some(self.selected_server(FieldId::VpnProfile, &self.vpn_ids, "VPN")?)
            } else {
                None
            },
            local_port: if mode.uses_ssh() {
                self.parse_port(FieldId::LocalPort)?
            } else {
                None
            },
        };
        let settings = ConnectionSettings {
            host: networked
                .then(|| self.form.optional(FieldId::Host))
                .flatten(),
            port: networked.then_some(port).flatten(),
            username: (!file_based)
                .then(|| self.form.optional(FieldId::Username))
                .flatten(),
            database: self.form.optional(FieldId::Database),
            file_path: file_based.then_some(file_path).flatten(),
            create_if_missing: file_based && self.form.toggle(FieldId::CreateIfMissing),
            connect_timeout_secs: timeout.clamp(1, 600),
            tls: TlsSettings {
                mode: TlsMode::ALL[self
                    .form
                    .choice(FieldId::TlsMode)
                    .min(TlsMode::ALL.len() - 1)],
                ca_certificate: self
                    .form
                    .optional(FieldId::CaCertificate)
                    .map(PathBuf::from),
            },
            save_password: self.form.toggle(FieldId::SavePassword),
            query_timeout_secs: query_timeout.filter(|seconds| *seconds > 0),
            properties: parse_properties(&self.form.text(FieldId::Properties))?,
        };
        self.check_required_properties(driver, &settings.properties)?;
        let definition = DataSourceDefinition {
            id: self.editing.unwrap_or_default(),
            name,
            driver: driver.id.clone(),
            environment: Environment::ALL[self
                .form
                .choice(FieldId::Environment)
                .min(Environment::ALL.len() - 1)],
            color,
            settings,
            network,
        };
        let password = (self.form.is_touched(FieldId::Password) || self.editing.is_none())
            .then(|| self.form.text(FieldId::Password))
            .filter(|password| !password.is_empty())
            .map(Secret::new);
        Ok((definition, password))
    }

    fn properties_hint(&self) -> String {
        let known = self
            .selected_driver()
            .map(|driver| driver.properties.as_slice())
            .unwrap_or_default();
        if known.is_empty() {
            return "key=value; key=value".into();
        }
        let mut names: Vec<String> = known
            .iter()
            .map(|property| {
                if property.required {
                    format!("{}=… (required)", property.name)
                } else {
                    format!("{}=…", property.name)
                }
            })
            .collect();
        names.sort_by_key(|name| !name.contains("required"));
        names.join("; ")
    }

    fn check_required_properties(
        &self,
        driver: &DriverDescriptor,
        properties: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        let missing: Vec<&str> = driver
            .properties
            .iter()
            .filter(|property| property.required)
            .filter(|property| {
                properties
                    .get(property.name)
                    .is_none_or(|value| value.trim().is_empty())
            })
            .map(|property| property.name)
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{} needs the properties: {}",
                driver.name,
                missing.join(", ")
            ))
        }
    }

    pub fn servers(&self) -> &ServerChoices {
        &self.servers
    }
}

impl FormView for DataSourceForm {
    fn title(&self) -> String {
        if self.editing.is_some() {
            "Edit data source".into()
        } else {
            "New data source".into()
        }
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
                    (FieldValue::Secret(input), FieldId::Password)
                        if input.is_empty()
                            && self.editing.is_some()
                            && !self.form.is_touched(FieldId::Password) =>
                    {
                        Some("(unchanged)".into())
                    }
                    (FieldValue::Text(input), FieldId::Properties) if input.is_empty() => {
                        Some(self.properties_hint())
                    }
                    (FieldValue::Text(input), FieldId::LocalPort) if input.is_empty() => {
                        Some("automatic".into())
                    }
                    (FieldValue::Text(input), FieldId::QueryTimeout) if input.is_empty() => {
                        Some("none".into())
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
    use relix_driver_api::DriverCapabilities;

    fn drivers() -> Vec<DriverDescriptor> {
        vec![
            DriverDescriptor::test(
                "postgres",
                "PostgreSQL",
                ConnectionTarget::Network { default_port: 5432 },
                DriverCapabilities::relational(),
            ),
            DriverDescriptor::test(
                "mysql",
                "MySQL",
                ConnectionTarget::Network { default_port: 3306 },
                DriverCapabilities::relational(),
            ),
            DriverDescriptor::test(
                "sqlite",
                "SQLite",
                ConnectionTarget::File,
                DriverCapabilities::relational(),
            ),
            DriverDescriptor {
                properties: vec![relix_driver_api::ConnectionProperty::required(
                    "project_id",
                    "Google Cloud project",
                )],
                ..DriverDescriptor::test(
                    "bigquery",
                    "BigQuery",
                    ConnectionTarget::Cloud,
                    DriverCapabilities::relational(),
                )
            },
        ]
    }

    fn servers() -> ServerChoices {
        ServerChoices {
            ssh: vec![ServerChoice {
                id: ServerConfigId::new(),
                label: "Production Bastion · global".into(),
            }],
            vpn: vec![ServerChoice {
                id: ServerConfigId::new(),
                label: "Office · project".into(),
            }],
        }
    }

    fn focus(form: &mut DataSourceForm, id: FieldId) {
        form.form.focused = form
            .form
            .fields
            .iter()
            .position(|field| field.id == id)
            .unwrap();
    }

    fn press(form: &mut DataSourceForm, code: KeyCode) {
        form.handle_key(KeyChord::new(code, Modifiers::NONE));
    }

    #[test]
    fn builds_network_definition_with_password() {
        let mut form = DataSourceForm::new(drivers(), None, servers());
        form.form.set_text(FieldId::Name, "Local");
        form.form.set_text(FieldId::Port, "");
        form.form.set_text(FieldId::Database, "app");
        form.form.set_text(FieldId::Password, "pw");
        form.form.set_text(
            FieldId::Properties,
            "application_name=relix; sslnegotiation = direct",
        );
        let (definition, password) = form.build().unwrap();
        assert_eq!(definition.driver.as_str(), "postgres");
        assert_eq!(definition.settings.port, Some(5432));
        assert_eq!(definition.settings.database.as_deref(), Some("app"));
        assert_eq!(
            definition.settings.property("sslnegotiation"),
            Some("direct")
        );
        assert_eq!(definition.network.mode, NetworkMode::Direct);
        assert_eq!(password, Some(Secret::new("pw")));
    }

    #[test]
    fn switching_engine_updates_default_port_and_visible_fields() {
        let mut form = DataSourceForm::new(drivers(), None, servers());
        form.form.set_text(FieldId::Port, "5432");
        focus(&mut form, FieldId::Driver);
        press(&mut form, KeyCode::Right);
        assert_eq!(form.form.text(FieldId::Port), "3306");
        press(&mut form, KeyCode::Right);
        assert!(form.is_visible(FieldId::FilePath));
        assert!(!form.is_visible(FieldId::Host));
        assert!(!form.is_visible(FieldId::NetworkMode));
        form.form.set_text(FieldId::Name, "Notes");
        assert_eq!(form.build().unwrap_err(), "a database file is required");
        form.form.set_text(FieldId::FilePath, "/tmp/notes.db");
        let (definition, _) = form.build().unwrap();
        assert_eq!(definition.settings.host, None);
    }

    #[test]
    fn ssh_mode_requires_and_stores_a_server_reference() {
        let servers = servers();
        let bastion = servers.ssh[0].id;
        let mut form = DataSourceForm::new(drivers(), None, servers);
        form.form.set_text(FieldId::Name, "Production");
        assert!(!form.is_visible(FieldId::SshConfig));
        focus(&mut form, FieldId::NetworkMode);
        press(&mut form, KeyCode::Right);
        assert!(form.is_visible(FieldId::SshConfig) && form.is_visible(FieldId::LocalPort));
        assert!(!form.is_visible(FieldId::VpnProfile));
        form.form.set_text(FieldId::LocalPort, "15432");
        let (definition, _) = form.build().unwrap();
        assert_eq!(definition.network.mode, NetworkMode::SshTunnel);
        assert_eq!(definition.network.ssh, Some(bastion));
        assert_eq!(definition.network.local_port, Some(15432));
        assert_eq!(definition.network.vpn, None);
    }

    #[test]
    fn missing_configurations_and_cloud_tunnels_are_rejected() {
        let mut existing = DataSourceDefinition::new("Main", DriverId::from("postgres"));
        existing.network.mode = NetworkMode::SshTunnel;
        existing.network.ssh = Some(ServerConfigId::new());
        let form = DataSourceForm::new(drivers(), Some(&existing), servers());
        assert_eq!(
            form.build().unwrap_err(),
            "select an existing SSH configuration"
        );

        let mut form = DataSourceForm::new(drivers(), None, servers());
        form.form.set_text(FieldId::Name, "Warehouse");
        focus(&mut form, FieldId::Driver);
        press(&mut form, KeyCode::Left);
        assert!(!form.is_visible(FieldId::Host));
        focus(&mut form, FieldId::NetworkMode);
        press(&mut form, KeyCode::Right);
        assert!(
            form.build()
                .unwrap_err()
                .contains("SSH tunnels cannot carry it")
        );
        form.form.set_choice(FieldId::NetworkMode, 0);
        assert!(
            form.build()
                .unwrap_err()
                .contains("needs the properties: project_id")
        );
        assert!(form.properties_hint().contains("project_id=… (required)"));
        form.form
            .set_text(FieldId::Properties, "project_id=analytics-prod");
        assert!(form.build().is_ok());
    }

    #[test]
    fn validation_errors() {
        let mut form = DataSourceForm::new(drivers(), None, ServerChoices::default());
        assert_eq!(form.build().unwrap_err(), "name is required");
        form.form.set_text(FieldId::Name, "x");
        form.form.set_text(FieldId::Port, "99999");
        assert!(form.build().unwrap_err().contains("invalid port"));
        form.form.set_text(FieldId::Port, "1");
        form.form.set_text(FieldId::Color, "blurple");
        assert!(form.build().unwrap_err().contains("invalid color"));
        form.form.set_text(FieldId::Color, "");
        form.form.set_text(FieldId::Properties, "novalue");
        assert!(form.build().unwrap_err().contains("key=value"));
    }

    #[test]
    fn editing_keeps_stored_password_and_unknown_fields() {
        let mut existing = DataSourceDefinition::new("Main", DriverId::from("postgres"));
        existing.settings.port = Some(5433);
        existing.settings.query_timeout_secs = Some(30);
        existing
            .settings
            .properties
            .insert("role".into(), "analyst".into());
        let form = DataSourceForm::new(drivers(), Some(&existing), servers());
        let (definition, password) = form.build().unwrap();
        assert_eq!(definition.id, existing.id);
        assert_eq!(definition.settings.port, Some(5433));
        assert_eq!(definition.settings.query_timeout_secs, Some(30));
        assert_eq!(definition.settings.property("role"), Some("analyst"));
        assert_eq!(password, None);
    }
}
