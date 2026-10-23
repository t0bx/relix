/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_config::DriverRegistrySettings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrySource {
    pub name: String,
    pub url: String,
    pub public_key: String,
}

impl RegistrySource {
    pub fn new(
        name: impl Into<String>,
        url: impl Into<String>,
        public_key: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            public_key: public_key.into(),
        }
    }
}

impl From<&DriverRegistrySettings> for RegistrySource {
    fn from(settings: &DriverRegistrySettings) -> Self {
        Self::new(&settings.name, &settings.url, &settings.public_key)
    }
}
