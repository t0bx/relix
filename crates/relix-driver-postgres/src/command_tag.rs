/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::StatementOutcome;
use relix_query::TopLevelWords;

const AFFECTING_VERBS: [&str; 5] = ["INSERT", "UPDATE", "DELETE", "MERGE", "COPY"];
const ROW_RETURNING_VERBS: [&str; 12] = [
    "SELECT", "VALUES", "TABLE", "SHOW", "EXPLAIN", "FETCH", "INSERT", "UPDATE", "DELETE", "MERGE",
    "CALL", "WITH",
];
const CTE_BODY_VERBS: [&str; 7] = [
    "SELECT", "INSERT", "UPDATE", "DELETE", "MERGE", "VALUES", "TABLE",
];
const OBJECT_MODIFIERS: [&str; 12] = [
    "OR",
    "REPLACE",
    "TEMP",
    "TEMPORARY",
    "UNLOGGED",
    "UNIQUE",
    "GLOBAL",
    "LOCAL",
    "TRUSTED",
    "PROCEDURAL",
    "RECURSIVE",
    "CONSTRAINT",
];
const MULTI_WORD_OBJECT_PREFIXES: [&str; 4] = ["MATERIALIZED", "FOREIGN", "EVENT", "DATA"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandInfo {
    verb: String,
    tag: String,
    streams_copy_data: bool,
}

impl CommandInfo {
    /// Reconstructs the command from the statement text, because the simple query protocol
    /// client only reports affected row counts and drops the server's command tag.
    pub(crate) fn from_words(statement: &TopLevelWords) -> Self {
        let keywords = statement.words.as_slice();
        let verb = main_verb(keywords);
        let streams_copy_data = verb == "COPY"
            && keywords
                .iter()
                .any(|word| word == "STDIN" || word == "STDOUT");
        Self {
            tag: command_tag(keywords, &verb),
            verb,
            streams_copy_data,
        }
    }

    pub(crate) fn may_return_rows(&self) -> bool {
        ROW_RETURNING_VERBS.contains(&self.verb.as_str())
    }

    pub(crate) fn streams_copy_data(&self) -> bool {
        self.streams_copy_data
    }

    pub(crate) fn outcome(&self, affected_rows: u64) -> StatementOutcome {
        if AFFECTING_VERBS.contains(&self.verb.as_str()) {
            StatementOutcome::Affected {
                rows: affected_rows,
            }
        } else {
            StatementOutcome::Completed {
                message: self.tag.clone(),
            }
        }
    }
}

fn main_verb(keywords: &[String]) -> String {
    match keywords.first().map(String::as_str) {
        Some("WITH") => keywords
            .iter()
            .skip(1)
            .find(|word| CTE_BODY_VERBS.contains(&word.as_str()))
            .cloned()
            .unwrap_or_else(|| "WITH".to_owned()),
        Some(first) => first.to_owned(),
        None => String::new(),
    }
}

fn command_tag(keywords: &[String], verb: &str) -> String {
    match verb {
        "" => "EMPTY QUERY".to_owned(),
        "CREATE" | "DROP" | "ALTER" => {
            object_command_tag(verb, keywords.get(1..).unwrap_or_default())
        }
        "START" => "START TRANSACTION".to_owned(),
        "END" => "COMMIT".to_owned(),
        "ABORT" => "ROLLBACK".to_owned(),
        "TRUNCATE" => "TRUNCATE TABLE".to_owned(),
        "REFRESH" => "REFRESH MATERIALIZED VIEW".to_owned(),
        other => other.to_owned(),
    }
}

fn object_command_tag(verb: &str, rest: &[String]) -> String {
    let words = rest
        .iter()
        .skip_while(|word| OBJECT_MODIFIERS.contains(&word.as_str()));
    let mut tag = verb.to_owned();
    for word in words {
        tag.push(' ');
        tag.push_str(word);
        if !MULTI_WORD_OBJECT_PREFIXES.contains(&word.as_str()) {
            break;
        }
    }
    tag
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::Dialect;
    use relix_query::top_level_words;

    fn info(sql: &str) -> CommandInfo {
        CommandInfo::from_words(&top_level_words(sql, Dialect::Postgres))
    }

    fn completed(message: &str) -> StatementOutcome {
        StatementOutcome::Completed {
            message: message.to_owned(),
        }
    }

    #[test]
    fn data_modifications_report_affected_rows() {
        for sql in [
            "insert into t values (1)",
            "UPDATE t SET a = 1",
            "delete from t",
            "with gone as (select 1) delete from t",
            "copy t from '/tmp/x.csv'",
        ] {
            assert_eq!(
                info(sql).outcome(3),
                StatementOutcome::Affected { rows: 3 },
                "{sql}"
            );
        }
    }

    #[test]
    fn ddl_and_transaction_control_complete_with_tags() {
        assert_eq!(
            info("create table t (a int)").outcome(0),
            completed("CREATE TABLE")
        );
        assert_eq!(
            info("create or replace view v as select 1").outcome(0),
            completed("CREATE VIEW")
        );
        assert_eq!(
            info("CREATE UNIQUE INDEX i ON t (a)").outcome(0),
            completed("CREATE INDEX")
        );
        assert_eq!(
            info("drop materialized view if exists m").outcome(0),
            completed("DROP MATERIALIZED VIEW")
        );
        assert_eq!(
            info("create foreign data wrapper w").outcome(0),
            completed("CREATE FOREIGN DATA WRAPPER")
        );
        assert_eq!(info("begin").outcome(0), completed("BEGIN"));
        assert_eq!(info("end").outcome(0), completed("COMMIT"));
        assert_eq!(info("set search_path = x").outcome(0), completed("SET"));
        assert_eq!(info("  -- nothing\n").outcome(0), completed("EMPTY QUERY"));
    }

    #[test]
    fn row_returning_statements_are_described() {
        assert!(info("select 1").may_return_rows());
        assert!(info("with x as (select 1) select * from x").may_return_rows());
        assert!(info("insert into t values (1) returning id").may_return_rows());
        assert!(!info("create table t (a int)").may_return_rows());
        assert!(!info("begin").may_return_rows());
    }

    #[test]
    fn copy_streams_are_detected() {
        assert!(info("COPY t FROM STDIN").streams_copy_data());
        assert!(info("copy (select 1) to stdout").streams_copy_data());
        assert!(!info("copy t from '/tmp/file'").streams_copy_data());
    }
}
