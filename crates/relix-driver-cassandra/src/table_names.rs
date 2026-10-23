/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{Dialect, TableRef};
use relix_driver_api::DriverError;

pub(crate) fn keyspace_of<'a>(
    table: &'a TableRef,
    default_keyspace: Option<&'a str>,
) -> Result<&'a str, DriverError> {
    table
        .database
        .as_deref()
        .or(table.schema.as_deref())
        .filter(|keyspace| !keyspace.is_empty())
        .or(default_keyspace)
        .ok_or_else(|| {
            DriverError::configuration(format!(
                "no keyspace given for table '{}' and the data source has no default keyspace",
                table.name
            ))
        })
}

pub(crate) fn quote(identifier: &str) -> String {
    Dialect::Cql.quote_identifier(identifier)
}

pub(crate) fn qualified_name(keyspace: &str, table: &str) -> String {
    format!("{}.{}", quote(keyspace), quote(table))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyspace_falls_back_to_the_default() {
        let qualified = TableRef::new(Some("shop".into()), None, "orders");
        assert_eq!(keyspace_of(&qualified, Some("other")).unwrap(), "shop");
        let bare = TableRef::new(None, None, "orders");
        assert_eq!(keyspace_of(&bare, Some("other")).unwrap(), "other");
        assert!(keyspace_of(&bare, None).is_err());
    }

    #[test]
    fn names_are_quoted_case_sensitively() {
        assert_eq!(
            qualified_name("Shop", "o\"rders"),
            "\"Shop\".\"o\"\"rders\""
        );
    }
}
