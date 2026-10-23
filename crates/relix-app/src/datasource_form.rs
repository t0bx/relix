/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::input::LineInput;
use crate::services::DriverDescriptor;
use relix_core::{
    ConnectionSettings, DataSourceDefinition, DataSourceId, DriverId, Environment, Secret, TlsMode,
    TlsSettings,
};
use relix_driver_api::ConnectionTarget;
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
    Timeout,
    TlsMode,
    CaCertificate,
    Environment,
    Color,
    SavePassword,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    Text(LineInput),
    Secret(LineInput),
    Choice {
        options: Vec<String>,
        selected: usize,
    },
    Toggle(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    pub id: FieldId,
    pub label: &'static str,
    pub value: FieldValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormStatus {
    pub message: String,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSourceForm {
    pub editing: Option<DataSourceId>,
    pub fields: Vec<FormField>,
    pub focused: usize,
    pub drivers: Vec<DriverDescriptor>,
    pub status: Option<FormStatus>,
    pub testing: bool,
    pub password_touched: bool,
}

fn text(id: FieldId, label: &'static str, value: &str) -> FormField {
    FormField {
        id,
        label,
        value: FieldValue::Text(LineInput::with_text(value)),
    }
}

fn choice(id: FieldId, label: &'static str, options: Vec<String>, selected: usize) -> FormField {
    FormField {
        id,
        label,
        value: FieldValue::Choice { options, selected },
    }
}

fn toggle(id: FieldId, label: &'static str, value: bool) -> FormField {
    FormField {
        id,
        label,
        value: FieldValue::Toggle(value),
    }
}

impl DataSourceForm {
    pub fn new(drivers: Vec<DriverDescriptor>, existing: Option<&DataSourceDefinition>) -> Self {
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
        let tls_index = TlsMode::ALL
            .iter()
            .position(|mode| *mode == settings.tls.mode)
            .unwrap_or(0);
        let environment_index = Environment::ALL
            .iter()
            .position(|environment| *environment == definition.environment)
            .unwrap_or(0);
        let fields = vec![
            text(FieldId::Name, "Name", &definition.name),
            choice(
                FieldId::Driver,
                "Engine",
                drivers.iter().map(|driver| driver.name.clone()).collect(),
                driver_index,
            ),
            text(
                FieldId::Host,
                "Host",
                settings.host.as_deref().unwrap_or("localhost"),
            ),
            text(
                FieldId::Port,
                "Port",
                &settings
                    .port
                    .or_else(|| {
                        drivers
                            .get(driver_index)
                            .and_then(|driver| driver.default_port)
                    })
                    .map(|port| port.to_string())
                    .unwrap_or_default(),
            ),
            text(
                FieldId::Username,
                "User",
                settings.username.as_deref().unwrap_or_default(),
            ),
            FormField {
                id: FieldId::Password,
                label: "Password",
                value: FieldValue::Secret(LineInput::default()),
            },
            text(
                FieldId::Database,
                "Database",
                settings.database.as_deref().unwrap_or_default(),
            ),
            text(
                FieldId::FilePath,
                "File",
                &settings
                    .file_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ),
            toggle(
                FieldId::CreateIfMissing,
                "Create file",
                settings.create_if_missing,
            ),
            text(
                FieldId::Timeout,
                "Timeout (s)",
                &settings.connect_timeout_secs.to_string(),
            ),
            choice(
                FieldId::TlsMode,
                "TLS",
                TlsMode::ALL
                    .iter()
                    .map(|mode| mode.label().to_owned())
                    .collect(),
                tls_index,
            ),
            text(
                FieldId::CaCertificate,
                "CA cert",
                &settings
                    .tls
                    .ca_certificate
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ),
            choice(
                FieldId::Environment,
                "Environment",
                Environment::ALL
                    .iter()
                    .map(|environment| environment.label().to_owned())
                    .collect(),
                environment_index,
            ),
            text(
                FieldId::Color,
                "Color",
                definition.color.as_deref().unwrap_or_default(),
            ),
            toggle(
                FieldId::SavePassword,
                "Save password",
                settings.save_password,
            ),
        ];
        Self {
            editing: existing.map(|definition| definition.id),
            fields,
            focused: 0,
            drivers,
            status: None,
            testing: false,
            password_touched: false,
        }
    }

    pub fn selected_driver(&self) -> Option<&DriverDescriptor> {
        let index = match self.field(FieldId::Driver).map(|field| &field.value) {
            Some(FieldValue::Choice { selected, .. }) => *selected,
            _ => 0,
        };
        self.drivers.get(index)
    }

    fn is_file_based(&self) -> bool {
        self.selected_driver()
            .is_some_and(|driver| driver.target == ConnectionTarget::File)
    }

    /// Fields relevant for the selected engine; file based engines hide network settings.
    pub fn is_visible(&self, id: FieldId) -> bool {
        let file = self.is_file_based();
        match id {
            FieldId::Host
            | FieldId::Port
            | FieldId::Username
            | FieldId::Password
            | FieldId::TlsMode
            | FieldId::CaCertificate
            | FieldId::SavePassword => !file,
            FieldId::FilePath | FieldId::CreateIfMissing => file,
            _ => true,
        }
    }

    pub fn visible_indexes(&self) -> Vec<usize> {
        self.fields
            .iter()
            .enumerate()
            .filter(|(_, field)| self.is_visible(field.id))
            .map(|(index, _)| index)
            .collect()
    }

    pub fn move_focus(&mut self, delta: isize) {
        let visible = self.visible_indexes();
        let position = visible
            .iter()
            .position(|index| *index == self.focused)
            .unwrap_or(0);
        let count = visible.len() as isize;
        let next = (position as isize + delta).rem_euclid(count.max(1)) as usize;
        self.focused = visible.get(next).copied().unwrap_or(0);
    }

    pub fn focused_field_mut(&mut self) -> Option<&mut FormField> {
        self.fields.get_mut(self.focused)
    }

    pub fn field(&self, id: FieldId) -> Option<&FormField> {
        self.fields.iter().find(|field| field.id == id)
    }

    fn field_mut(&mut self, id: FieldId) -> Option<&mut FormField> {
        self.fields.iter_mut().find(|field| field.id == id)
    }

    fn text_of(&self, id: FieldId) -> String {
        match self.field(id).map(|field| &field.value) {
            Some(FieldValue::Text(input) | FieldValue::Secret(input)) => {
                input.text().trim().to_owned()
            }
            _ => String::new(),
        }
    }

    fn choice_of(&self, id: FieldId) -> usize {
        match self.field(id).map(|field| &field.value) {
            Some(FieldValue::Choice { selected, .. }) => *selected,
            _ => 0,
        }
    }

    fn toggle_of(&self, id: FieldId) -> bool {
        matches!(
            self.field(id).map(|field| &field.value),
            Some(FieldValue::Toggle(true))
        )
    }

    /// Cycles a choice field. Switching the engine fills in its default port when the port
    /// is empty or still the previous engine's default.
    pub fn cycle_choice(&mut self, delta: isize) {
        let previous_port = self
            .selected_driver()
            .and_then(|driver| driver.default_port);
        let focused_id = self.fields.get(self.focused).map(|field| field.id);
        if let Some(FormField {
            value: FieldValue::Choice { options, selected },
            ..
        }) = self.fields.get_mut(self.focused)
            && !options.is_empty()
        {
            let count = options.len() as isize;
            *selected = (*selected as isize + delta).rem_euclid(count) as usize;
        }
        if focused_id == Some(FieldId::Driver) {
            let current_port = self.text_of(FieldId::Port);
            let new_port = self
                .selected_driver()
                .and_then(|driver| driver.default_port);
            let replace = current_port.is_empty()
                || previous_port.map(|port| port.to_string()) == Some(current_port);
            if replace
                && let Some(FormField {
                    value: FieldValue::Text(input),
                    ..
                }) = self.field_mut(FieldId::Port)
            {
                input.set(&new_port.map(|port| port.to_string()).unwrap_or_default());
            }
        }
    }

    pub fn toggle_focused(&mut self) {
        if let Some(FormField {
            value: FieldValue::Toggle(value),
            ..
        }) = self.fields.get_mut(self.focused)
        {
            *value = !*value;
        }
    }

    /// Validates the form and produces the definition plus the password to store. An
    /// untouched password field while editing means "keep the stored password".
    pub fn build(&self) -> Result<(DataSourceDefinition, Option<Secret>), String> {
        let driver = self
            .selected_driver()
            .ok_or("no database engine available")?;
        let name = self.text_of(FieldId::Name);
        if name.is_empty() {
            return Err("name is required".into());
        }
        let optional = |id| Some(self.text_of(id)).filter(|value| !value.is_empty());
        let port = match optional(FieldId::Port) {
            Some(port) => Some(
                port.parse::<u16>()
                    .map_err(|_| format!("invalid port '{port}'"))?,
            ),
            None => driver.default_port,
        };
        let timeout = self
            .text_of(FieldId::Timeout)
            .parse::<u64>()
            .map_err(|_| "timeout must be a number of seconds".to_owned())?;
        let file_based = driver.target == ConnectionTarget::File;
        let file_path = optional(FieldId::FilePath).map(PathBuf::from);
        if file_based && file_path.is_none() {
            return Err("a database file is required".into());
        }
        let color = optional(FieldId::Color);
        if let Some(color) = &color
            && color.parse::<relix_config::ColorSpec>().is_err()
        {
            return Err(format!("invalid color '{color}'"));
        }
        let settings = ConnectionSettings {
            host: (!file_based).then(|| optional(FieldId::Host)).flatten(),
            port: (!file_based).then_some(port).flatten(),
            username: (!file_based).then(|| optional(FieldId::Username)).flatten(),
            database: optional(FieldId::Database),
            file_path: file_based.then_some(file_path).flatten(),
            create_if_missing: file_based && self.toggle_of(FieldId::CreateIfMissing),
            connect_timeout_secs: timeout.clamp(1, 600),
            tls: TlsSettings {
                mode: TlsMode::ALL[self.choice_of(FieldId::TlsMode).min(TlsMode::ALL.len() - 1)],
                ca_certificate: optional(FieldId::CaCertificate).map(PathBuf::from),
            },
            save_password: self.toggle_of(FieldId::SavePassword),
            ..ConnectionSettings::default()
        };
        let definition = DataSourceDefinition {
            id: self.editing.unwrap_or_default(),
            name,
            driver: driver.id.clone(),
            environment: Environment::ALL[self
                .choice_of(FieldId::Environment)
                .min(Environment::ALL.len() - 1)],
            color,
            settings,
            network: Default::default(),
        };
        let password = (self.password_touched || self.editing.is_none())
            .then(|| self.text_of(FieldId::Password))
            .filter(|password| !password.is_empty())
            .map(Secret::new);
        Ok((definition, password))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        ]
    }

    fn set_text(form: &mut DataSourceForm, id: FieldId, value: &str) {
        if let Some(FormField {
            value: FieldValue::Text(input) | FieldValue::Secret(input),
            ..
        }) = form.field_mut(id)
        {
            input.set(value);
        }
    }

    #[test]
    fn builds_network_definition_with_password() {
        let mut form = DataSourceForm::new(drivers(), None);
        set_text(&mut form, FieldId::Name, "Local");
        set_text(&mut form, FieldId::Port, "");
        set_text(&mut form, FieldId::Database, "app");
        set_text(&mut form, FieldId::Password, "pw");
        let (definition, password) = form.build().unwrap();
        assert_eq!(definition.driver.as_str(), "postgres");
        assert_eq!(definition.settings.port, Some(5432));
        assert_eq!(definition.settings.database.as_deref(), Some("app"));
        assert_eq!(password, Some(Secret::new("pw")));
    }

    #[test]
    fn switching_engine_updates_default_port_and_visible_fields() {
        let mut form = DataSourceForm::new(drivers(), None);
        set_text(&mut form, FieldId::Port, "5432");
        form.focused = 1;
        form.cycle_choice(1);
        assert_eq!(form.text_of(FieldId::Port), "3306");
        form.cycle_choice(1);
        assert!(form.is_visible(FieldId::FilePath));
        assert!(!form.is_visible(FieldId::Host));
        set_text(&mut form, FieldId::Name, "Notes");
        assert_eq!(form.build().unwrap_err(), "a database file is required");
        set_text(&mut form, FieldId::FilePath, "/tmp/notes.db");
        let (definition, _) = form.build().unwrap();
        assert_eq!(definition.settings.host, None);
    }

    #[test]
    fn new_forms_prefill_the_engine_default_port() {
        let form = DataSourceForm::new(drivers(), None);
        assert_eq!(form.text_of(FieldId::Port), "5432");
    }

    #[test]
    fn validation_errors() {
        let mut form = DataSourceForm::new(drivers(), None);
        assert_eq!(form.build().unwrap_err(), "name is required");
        set_text(&mut form, FieldId::Name, "x");
        set_text(&mut form, FieldId::Port, "99999");
        assert!(form.build().unwrap_err().contains("invalid port"));
        set_text(&mut form, FieldId::Port, "1");
        set_text(&mut form, FieldId::Color, "blurple");
        assert!(form.build().unwrap_err().contains("invalid color"));
    }

    #[test]
    fn editing_keeps_stored_password_unless_touched() {
        let mut existing = DataSourceDefinition::new("Main", DriverId::from("postgres"));
        existing.settings.port = Some(5433);
        let form = DataSourceForm::new(drivers(), Some(&existing));
        let (definition, password) = form.build().unwrap();
        assert_eq!(definition.id, existing.id);
        assert_eq!(definition.settings.port, Some(5433));
        assert_eq!(password, None);
    }
}
