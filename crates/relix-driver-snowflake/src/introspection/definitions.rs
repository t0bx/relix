/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::SnowflakeConnection;
use crate::introspection::{identifier, scope, show, string_literal};
use relix_core::{ObjectInfo, ObjectKind};
use relix_driver_api::DriverError;

/// Returns the DDL of an object through `GET_DDL`. Functions and procedures are
/// addressed by signature, so every overload found by `SHOW … LIKE` contributes its DDL.
pub(crate) async fn ddl(
    connection: &SnowflakeConnection,
    object: &ObjectInfo,
) -> Result<Option<String>, DriverError> {
    let qualified = connection.builder.qualified_table(&object.object);
    let targets = match object.kind {
        ObjectKind::Table => vec![("TABLE", qualified)],
        ObjectKind::View | ObjectKind::MaterializedView => vec![("VIEW", qualified)],
        ObjectKind::Sequence => vec![("SEQUENCE", qualified)],
        ObjectKind::Function => {
            signatures(connection, object, "USER FUNCTIONS", "FUNCTION").await?
        }
        ObjectKind::Procedure => signatures(connection, object, "PROCEDURES", "PROCEDURE").await?,
        _ => return Ok(None),
    };
    let mut statements = Vec::new();
    for (kind, name) in targets {
        let sql = format!(
            "SELECT GET_DDL({}, {})",
            string_literal(kind),
            string_literal(&name)
        );
        if let Some(statement) = connection.scalar(&sql).await? {
            statements.push(statement);
        }
    }
    Ok((!statements.is_empty()).then(|| statements.join("\n\n")))
}

async fn signatures(
    connection: &SnowflakeConnection,
    object: &ObjectInfo,
    listing: &str,
    kind: &'static str,
) -> Result<Vec<(&'static str, String)>, DriverError> {
    let (database, schema) = scope(&object.object)?;
    let sql = format!(
        "SHOW {listing} LIKE {} IN SCHEMA {}.{}",
        string_literal(&object.object.name),
        identifier(&database),
        identifier(&schema)
    );
    let qualified = connection.builder.qualified_table(&object.object);
    let routines = show(connection, &sql).await?;
    Ok(routines
        .rows()
        .filter(|row| row.get("name") == Some(object.object.name.as_str()))
        .filter_map(|row| argument_types(row.get("arguments")?))
        .map(|arguments| (kind, format!("{qualified}{arguments}")))
        .collect())
}

/// Extracts the parenthesised argument type list from the `arguments` column of
/// `SHOW FUNCTIONS`/`SHOW PROCEDURES` (`ADD(NUMBER, [VARCHAR]) RETURN NUMBER` →
/// `(NUMBER, VARCHAR)`); square brackets mark optional arguments.
fn argument_types(arguments: &str) -> Option<String> {
    let start = arguments.find('(')?;
    let mut depth = 0usize;
    for (offset, ch) in arguments[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let list = &arguments[start..=start + offset];
                    return Some(list.replace(['[', ']'], ""));
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argument_lists_are_taken_from_the_signature() {
        assert_eq!(
            argument_types("ADD_ONE(NUMBER(38,0), [VARCHAR]) RETURN NUMBER").as_deref(),
            Some("(NUMBER(38,0), VARCHAR)")
        );
        assert_eq!(
            argument_types("NOW() RETURN TIMESTAMP_LTZ").as_deref(),
            Some("()")
        );
        assert_eq!(argument_types("BROKEN(NUMBER"), None);
    }
}
