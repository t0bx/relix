/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::ordered_json::parse_document;
use mongodb::bson::{Bson, Document};
use relix_driver_api::DriverError;

const DATABASE_FIELD: &str = "$db";
const INVALID_DATABASE_CHARACTERS: [char; 7] = ['/', '\\', '.', ' ', '"', '$', '\0'];
const MAX_DATABASE_NAME_BYTES: usize = 63;

#[derive(Debug, Clone, PartialEq)]
pub enum ConsoleStatement {
    Use(String),
    Command {
        database: Option<String>,
        command: Document,
    },
}

/// Parses one console statement: either `use <database>` (switches the database later
/// commands run against) or a single command document in Extended JSON whose first field
/// is the command name. An optional `"$db"` field runs that one command against another
/// database. A trailing semicolon is ignored.
pub fn parse_statement(text: &str) -> Result<ConsoleStatement, DriverError> {
    let leading = text.len() - text.trim_start().len();
    let body = text.trim().trim_end_matches(';').trim_end();
    if body.is_empty() {
        return Err(DriverError::query("empty statement"));
    }
    if let Some(database) = use_target(body) {
        validate_database_name(database)?;
        return Ok(ConsoleStatement::Use(database.to_owned()));
    }
    let mut command = parse_document(body).map_err(|error| match error.position {
        Some(position) => {
            let offset = text[..leading].chars().count();
            error.with_position(position + offset)
        }
        None => error,
    })?;
    if command.is_empty() {
        return Err(DriverError::query(
            "the command document is empty: start it with the command name, e.g. {\"ping\": 1}",
        ));
    }
    let database = match command.remove(DATABASE_FIELD) {
        None => None,
        Some(Bson::String(name)) => {
            validate_database_name(&name)?;
            Some(name)
        }
        Some(_) => return Err(DriverError::query("\"$db\" must be a database name string")),
    };
    Ok(ConsoleStatement::Command { database, command })
}

fn use_target(body: &str) -> Option<&str> {
    let (keyword, rest) = body.split_once(char::is_whitespace)?;
    if !keyword.eq_ignore_ascii_case("use") {
        return None;
    }
    let target = rest.trim();
    (!target.starts_with('{')).then_some(target)
}

pub fn validate_database_name(name: &str) -> Result<(), DriverError> {
    let valid = !name.is_empty()
        && name.len() <= MAX_DATABASE_NAME_BYTES
        && !name.contains(INVALID_DATABASE_CHARACTERS);
    if valid {
        Ok(())
    } else {
        Err(DriverError::query(format!(
            "\"{name}\" is not a valid MongoDB database name"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;
    use pretty_assertions::assert_eq;

    #[test]
    fn use_switches_the_database() {
        assert_eq!(
            parse_statement("  USE shop ;").unwrap(),
            ConsoleStatement::Use("shop".into())
        );
        assert!(parse_statement("use my db").is_err());
    }

    #[test]
    fn commands_may_target_another_database() {
        let parsed = parse_statement(r#"{"ping": 1, "$db": "admin"};"#).unwrap();
        assert_eq!(
            parsed,
            ConsoleStatement::Command {
                database: Some("admin".into()),
                command: doc! { "ping": 1 },
            }
        );
    }

    #[test]
    fn syntax_errors_point_into_the_original_text() {
        let error = parse_statement("\n  {\"find\": }").unwrap_err();
        assert_eq!(error.position, Some(12));
        assert!(parse_statement(" ; ").is_err());
        assert!(parse_statement("{}").is_err());
        assert!(parse_statement(r#"{"ping": 1, "$db": 5}"#).is_err());
    }
}
