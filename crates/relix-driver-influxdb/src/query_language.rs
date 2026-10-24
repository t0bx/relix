/*
 * @author t0bx
 * @since 24.10.2026
 */

use relix_core::ConnectionSettings;
use relix_driver_api::DriverError;

pub(crate) const LANGUAGE_PROPERTY: &str = "query_language";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryLanguage {
    Sql,
    InfluxQl,
}

impl QueryLanguage {
    pub(crate) fn from_settings(settings: &ConnectionSettings) -> Result<Self, DriverError> {
        match settings
            .property(LANGUAGE_PROPERTY)
            .map(str::to_lowercase)
            .as_deref()
        {
            None | Some("sql") => Ok(Self::Sql),
            Some("influxql") => Ok(Self::InfluxQl),
            Some(other) => Err(DriverError::configuration(format!(
                "unknown {LANGUAGE_PROPERTY} '{other}': use 'sql' or 'influxql'"
            ))),
        }
    }

    pub(crate) fn endpoint(self) -> &'static str {
        match self {
            Self::Sql => "/api/v3/query_sql",
            Self::InfluxQl => "/api/v3/query_influxql",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_is_chosen_by_property() {
        let mut settings = ConnectionSettings::default();
        assert_eq!(
            QueryLanguage::from_settings(&settings).unwrap(),
            QueryLanguage::Sql
        );
        settings
            .properties
            .insert(LANGUAGE_PROPERTY.into(), "InfluxQL".into());
        let language = QueryLanguage::from_settings(&settings).unwrap();
        assert_eq!(language.endpoint(), "/api/v3/query_influxql");
        settings
            .properties
            .insert(LANGUAGE_PROPERTY.into(), "flux".into());
        assert!(QueryLanguage::from_settings(&settings).is_err());
    }
}
