/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::statement_words::TopLevelWords;

const DEFINITION_VERBS: [&str; 3] = ["CREATE", "DROP", "ALTER"];
const DEFINITION_OBJECTS: [&str; 18] = [
    "TABLE",
    "VIEW",
    "INDEX",
    "SCHEMA",
    "DATABASE",
    "CATALOG",
    "SEQUENCE",
    "FUNCTION",
    "MACRO",
    "TYPE",
    "TRIGGER",
    "PROCEDURE",
    "DICTIONARY",
    "USER",
    "ROLE",
    "SECRET",
    "QUOTA",
    "POLICY",
];

/// Builds the status message shown for a statement that completed without a result
/// set: the leading verb, followed by the object kind for definitions
/// (`CREATE TABLE`, `DROP MATERIALIZED VIEW`), or `OK` when there is no statement.
pub fn completion_message(words: &TopLevelWords) -> String {
    let Some(verb) = words.first() else {
        return "OK".to_owned();
    };
    if !DEFINITION_VERBS.contains(&verb) {
        return verb.to_owned();
    }
    let rest = &words.words[1..];
    let Some(position) = rest
        .iter()
        .position(|word| DEFINITION_OBJECTS.contains(&word.as_str()))
    else {
        return verb.to_owned();
    };
    let object = &rest[position];
    let materialized = position > 0 && rest[position - 1] == "MATERIALIZED";
    if materialized {
        format!("{verb} MATERIALIZED {object}")
    } else {
        format!("{verb} {object}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::statement_words::top_level_words;
    use relix_core::Dialect;

    fn message(sql: &str) -> String {
        completion_message(&top_level_words(sql, Dialect::Generic))
    }

    #[test]
    fn definitions_name_the_object_kind() {
        assert_eq!(
            message("create or replace temp table t (a int)"),
            "CREATE TABLE"
        );
        assert_eq!(
            message("DROP MATERIALIZED VIEW IF EXISTS mv"),
            "DROP MATERIALIZED VIEW"
        );
        assert_eq!(message("alter something odd"), "ALTER");
    }

    #[test]
    fn other_statements_report_their_verb() {
        assert_eq!(message("begin transaction"), "BEGIN");
        assert_eq!(message("SET search_path = x"), "SET");
        assert_eq!(message(" -- nothing\n"), "OK");
    }
}
