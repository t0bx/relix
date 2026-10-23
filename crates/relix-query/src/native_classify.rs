/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::classify::{StatementKind, StatementTraits};
use crate::lexer::{significant, tokenize};
use relix_core::Dialect;

pub(crate) fn classify_native(statement: &str, dialect: Dialect) -> Option<StatementTraits> {
    match dialect {
        Dialect::Cypher => Some(classify_cypher(statement)),
        Dialect::MongoCommand => Some(classify_mongo(statement)),
        Dialect::SearchRequest => Some(classify_search(statement)),
        _ => None,
    }
}

const MONGO_COMMANDS: &[&str] = &[
    "find",
    "aggregate",
    "count",
    "distinct",
    "listCollections",
    "listIndexes",
    "listDatabases",
    "collStats",
    "dbStats",
    "explain",
    "ping",
    "buildInfo",
    "serverStatus",
    "hello",
    "insert",
    "update",
    "findAndModify",
    "delete",
    "drop",
    "dropDatabase",
    "dropIndexes",
    "create",
    "createIndexes",
    "collMod",
    "renameCollection",
];

fn traits(kind: StatementKind, destructive: bool) -> StatementTraits {
    StatementTraits {
        kind,
        destructive,
        writes: !matches!(kind, StatementKind::Query | StatementKind::Session),
    }
}

fn classify_cypher(statement: &str) -> StatementTraits {
    let tokens = tokenize(statement, Dialect::Cypher);
    let has = |keyword: &str| significant(&tokens).any(|token| token.is_keyword(keyword));
    let has_where = has("WHERE");
    if has("DROP") {
        return traits(StatementKind::Definition, true);
    }
    if has("DELETE") {
        return traits(StatementKind::Delete, has("DETACH") || !has_where);
    }
    if has("SET") || has("REMOVE") {
        return traits(StatementKind::Update, !has_where);
    }
    if has("CREATE") || has("MERGE") {
        return traits(StatementKind::Insert, false);
    }
    traits(StatementKind::Query, false)
}

fn classify_mongo(statement: &str) -> StatementTraits {
    let document = serde_json::from_str::<serde_json::Value>(statement).ok();
    let Some(command) = document.as_ref().and_then(serde_json::Value::as_object) else {
        let session = statement
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("use ");
        return traits(
            if session {
                StatementKind::Session
            } else {
                StatementKind::Other
            },
            false,
        );
    };
    let name = MONGO_COMMANDS
        .iter()
        .find(|name| command.contains_key(**name))
        .copied()
        .unwrap_or_default();
    match name {
        "find" | "aggregate" | "count" | "distinct" | "listCollections" | "listIndexes"
        | "listDatabases" | "collStats" | "dbStats" | "explain" | "ping" | "buildInfo"
        | "serverStatus" | "hello" => traits(StatementKind::Query, false),
        "insert" => traits(StatementKind::Insert, false),
        "update" | "findAndModify" => {
            traits(StatementKind::Update, has_unfiltered(command, "updates"))
        }
        "delete" => traits(StatementKind::Delete, has_unfiltered(command, "deletes")),
        "drop" | "dropDatabase" | "dropIndexes" => traits(StatementKind::Definition, true),
        "create" | "createIndexes" | "collMod" | "renameCollection" => {
            traits(StatementKind::Definition, false)
        }
        _ => traits(StatementKind::Other, false),
    }
}

fn has_unfiltered(command: &serde_json::Map<String, serde_json::Value>, list: &str) -> bool {
    command
        .get(list)
        .and_then(serde_json::Value::as_array)
        .is_some_and(|operations| {
            operations.iter().any(|operation| {
                operation
                    .get("q")
                    .and_then(serde_json::Value::as_object)
                    .is_some_and(serde_json::Map::is_empty)
            })
        })
}

fn classify_search(statement: &str) -> StatementTraits {
    let mut words = statement.split_whitespace();
    let method = words.next().unwrap_or_default().to_ascii_uppercase();
    let path = words.next().unwrap_or_default().to_ascii_lowercase();
    let read_endpoint = [
        "_search",
        "_count",
        "_msearch",
        "_mapping",
        "_settings",
        "_cat",
        "_cluster",
        "_explain",
        "_validate",
    ]
    .iter()
    .any(|endpoint| path.contains(endpoint));
    match method.as_str() {
        "GET" | "HEAD" => traits(StatementKind::Query, false),
        "POST" if read_endpoint && !path.contains("_delete_by_query") => {
            traits(StatementKind::Query, false)
        }
        "DELETE" => traits(StatementKind::Delete, true),
        "POST" if path.contains("_delete_by_query") => traits(StatementKind::Delete, true),
        "POST" if path.contains("_update_by_query") => traits(StatementKind::Update, true),
        "PUT" | "POST" | "PATCH" => traits(StatementKind::Other, false),
        _ => traits(StatementKind::Other, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cypher_writes_and_unbounded_deletes() {
        let read = classify_cypher("MATCH (n:Person) RETURN n LIMIT 10");
        assert!(!read.writes);
        assert!(classify_cypher("MATCH (n) DETACH DELETE n").destructive);
        assert!(!classify_cypher("MATCH (n:Person) WHERE n.id = 1 SET n.name = 'x'").destructive);
        assert!(classify_cypher("CREATE (n:Person {name: 'ada'})").writes);
    }

    #[test]
    fn mongo_commands_by_name() {
        assert!(!classify_mongo(r#"{"find": "orders", "filter": {}}"#).writes);
        assert!(
            classify_mongo(r#"{"delete": "orders", "deletes": [{"q": {}, "limit": 0}]}"#)
                .destructive
        );
        assert!(
            !classify_mongo(r#"{"delete": "orders", "deletes": [{"q": {"_id": 1}, "limit": 1}]}"#)
                .destructive
        );
        assert!(classify_mongo(r#"{"drop": "orders"}"#).destructive);
        assert_eq!(classify_mongo("use shop").kind, StatementKind::Session);
    }

    #[test]
    fn search_requests_by_method_and_endpoint() {
        assert!(!classify_search("POST /logs/_search\n{}").writes);
        assert!(classify_search("DELETE /logs").destructive);
        assert!(classify_search("POST /logs/_delete_by_query\n{}").destructive);
        assert!(classify_search("PUT /logs/_doc/1\n{}").writes);
    }
}
