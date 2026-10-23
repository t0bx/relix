/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::DriverError;

const SNOWFLAKE_DOMAIN: &str = ".snowflakecomputing.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Account {
    identifier: String,
}

impl Account {
    /// Resolves the account from the `account` property, or from an account URL host such
    /// as `myorg-myaccount.snowflakecomputing.com` when the property is missing.
    pub(crate) fn resolve(property: Option<&str>, host: Option<&str>) -> Result<Self, DriverError> {
        let identifier = property
            .map(str::to_owned)
            .or_else(|| host.and_then(account_from_host))
            .ok_or_else(|| {
                DriverError::configuration(
                    "set the \"account\" property to the Snowflake account identifier \
                     (e.g. myorg-myaccount)",
                )
            })?;
        Ok(Self {
            identifier: identifier.trim().to_owned(),
        })
    }

    pub(crate) fn default_host(&self) -> String {
        format!("{}{SNOWFLAKE_DOMAIN}", self.identifier.to_lowercase())
    }

    /// Account name used in key-pair JWT claims: upper case and without the region or
    /// cloud suffix that legacy account locators carry (`xy12345.eu-central-1` → `XY12345`).
    pub(crate) fn jwt_name(&self) -> String {
        let name = self
            .identifier
            .split('.')
            .next()
            .unwrap_or(&self.identifier);
        name.to_uppercase()
    }
}

fn account_from_host(host: &str) -> Option<String> {
    let lower = host.trim().to_lowercase();
    let account = lower.strip_suffix(SNOWFLAKE_DOMAIN)?;
    let account = account.strip_suffix(".privatelink").unwrap_or(account);
    (!account.is_empty()).then(|| account.to_owned())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn org_accounts_keep_their_hyphen() {
        let account = Account::resolve(Some("myorg-myaccount"), None).unwrap();
        assert_eq!(account.jwt_name(), "MYORG-MYACCOUNT");
        assert_eq!(
            account.default_host(),
            "myorg-myaccount.snowflakecomputing.com"
        );
    }

    #[test]
    fn legacy_locators_drop_the_region_in_claims() {
        let account = Account::resolve(Some("xy12345.eu-central-1"), None).unwrap();
        assert_eq!(account.jwt_name(), "XY12345");
        assert_eq!(
            account.default_host(),
            "xy12345.eu-central-1.snowflakecomputing.com"
        );
    }

    #[test]
    fn account_falls_back_to_the_host_name() {
        let account =
            Account::resolve(None, Some("Acme-Prod.privatelink.snowflakecomputing.com")).unwrap();
        assert_eq!(account.jwt_name(), "ACME-PROD");
        assert!(Account::resolve(None, Some("db.internal")).is_err());
        assert!(Account::resolve(None, None).is_err());
    }
}
