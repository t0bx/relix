/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::ConnectionSettings;
use relix_driver_api::DriverError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchOptions {
    pub show_hidden: bool,
    pub exact_totals: bool,
}

impl SearchOptions {
    pub fn from_settings(settings: &ConnectionSettings) -> Result<Self, DriverError> {
        Ok(Self {
            show_hidden: flag(settings, "show_hidden")?,
            exact_totals: flag(settings, "track_total_hits")?,
        })
    }

    pub fn expand_wildcards(self) -> &'static str {
        if self.show_hidden {
            "all"
        } else {
            "open,closed"
        }
    }
}

fn flag(settings: &ConnectionSettings, name: &str) -> Result<bool, DriverError> {
    match settings
        .property(name)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        None | Some("false" | "no" | "0" | "off") => Ok(false),
        Some("true" | "yes" | "1" | "on") => Ok(true),
        Some(other) => Err(DriverError::configuration(format!(
            "property {name} must be true or false, got \"{other}\""
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_parse_booleans() {
        let mut settings = ConnectionSettings::default();
        assert_eq!(
            SearchOptions::from_settings(&settings).unwrap(),
            SearchOptions::default()
        );
        settings
            .properties
            .insert("show_hidden".into(), "TRUE".into());
        settings
            .properties
            .insert("track_total_hits".into(), "1".into());
        let options = SearchOptions::from_settings(&settings).unwrap();
        assert!(options.show_hidden && options.exact_totals);
        assert_eq!(options.expand_wildcards(), "all");
        settings
            .properties
            .insert("show_hidden".into(), "maybe".into());
        assert!(SearchOptions::from_settings(&settings).is_err());
    }
}
