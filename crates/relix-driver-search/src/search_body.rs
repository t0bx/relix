/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::DocumentQuery;
use relix_driver_api::DriverError;
use serde_json::{Map, Value as Json, json};

const TIEBREAK_FIELDS: [&str; 2] = ["_doc", "_shard_doc"];

pub fn search_body(query: &DocumentQuery, size: u64) -> Result<Map<String, Json>, DriverError> {
    let mut body = Map::new();
    body.insert("query".to_owned(), query_clause(query.filter.as_ref())?);
    body.insert(
        "sort".to_owned(),
        Json::Array(sort_clause(query.sort.as_ref())?),
    );
    body.insert("size".to_owned(), Json::from(size));
    body.insert("seq_no_primary_term".to_owned(), Json::Bool(true));
    if let Some(source) = source_clause(query.projection.as_ref())? {
        body.insert("_source".to_owned(), source);
    }
    Ok(body)
}

/// The filter is a Query DSL `query` object; an empty filter means `match_all`. A
/// filter wrapped as `{"query": {...}}` is unwrapped for convenience.
pub fn query_clause(filter: Option<&Json>) -> Result<Json, DriverError> {
    match filter {
        None | Some(Json::Null) => Ok(json!({"match_all": {}})),
        Some(Json::Object(clause)) if clause.is_empty() => Ok(json!({"match_all": {}})),
        Some(Json::Object(clause)) if clause.len() == 1 && clause.contains_key("query") => {
            query_clause(clause.get("query"))
        }
        Some(clause @ Json::Object(_)) => Ok(clause.clone()),
        Some(_) => Err(DriverError::query(
            "the filter must be a Query DSL object such as {\"match\": {\"field\": \"text\"}}",
        )),
    }
}

/// Accepts an Elasticsearch sort array, a single sort entry, a field name, or a
/// `{"field": 1 | -1 | "asc" | "desc"}` object, and always ends in a `_doc` tiebreak so
/// pages are stable and `search_after` has a total order to continue from.
pub fn sort_clause(sort: Option<&Json>) -> Result<Vec<Json>, DriverError> {
    let mut entries = match sort {
        None | Some(Json::Null) => Vec::new(),
        Some(Json::Array(entries)) => entries.clone(),
        Some(Json::String(field)) => vec![Json::from(field.clone())],
        Some(Json::Object(fields)) => fields
            .iter()
            .map(|(field, order)| sort_entry(field, order))
            .collect::<Result<_, _>>()?,
        Some(_) => {
            return Err(DriverError::query(
                "the sort must be a JSON array or object",
            ));
        }
    };
    if entries.is_empty() {
        entries.push(json!({"_score": "desc"}));
    }
    if !entries.iter().any(is_tiebreak) {
        entries.push(json!({"_doc": "asc"}));
    }
    Ok(entries)
}

fn sort_entry(field: &str, order: &Json) -> Result<Json, DriverError> {
    let direction = match order {
        Json::Number(number) if number.as_i64() == Some(1) => "asc",
        Json::Number(number) if number.as_i64() == Some(-1) => "desc",
        Json::String(text) if text.eq_ignore_ascii_case("asc") => "asc",
        Json::String(text) if text.eq_ignore_ascii_case("desc") => "desc",
        Json::Object(_) => return Ok(json!({ field: order })),
        _ => {
            return Err(DriverError::query(format!(
                "sort direction for {field} must be 1, -1, \"asc\" or \"desc\""
            )));
        }
    };
    Ok(json!({ field: direction }))
}

fn is_tiebreak(entry: &Json) -> bool {
    match entry {
        Json::String(field) => TIEBREAK_FIELDS.contains(&field.as_str()),
        Json::Object(fields) => fields
            .keys()
            .any(|field| TIEBREAK_FIELDS.contains(&field.as_str())),
        _ => false,
    }
}

/// Maps the projection to `_source` filtering: an array of field patterns, an object
/// with `includes`/`excludes`, a `{"field": 1 | 0}` object, or a boolean.
pub fn source_clause(projection: Option<&Json>) -> Result<Option<Json>, DriverError> {
    let Some(projection) = projection else {
        return Ok(None);
    };
    match projection {
        Json::Null => Ok(None),
        Json::Object(fields) if fields.is_empty() => Ok(None),
        Json::Bool(_) | Json::String(_) | Json::Array(_) => Ok(Some(projection.clone())),
        Json::Object(fields)
            if fields
                .keys()
                .all(|key| key == "includes" || key == "excludes") =>
        {
            Ok(Some(projection.clone()))
        }
        Json::Object(fields) => {
            let mut includes = Vec::new();
            let mut excludes = Vec::new();
            for (field, flag) in fields {
                match flag {
                    Json::Bool(true) => includes.push(field.clone()),
                    Json::Bool(false) => excludes.push(field.clone()),
                    Json::Number(number) if number.as_i64() == Some(1) => {
                        includes.push(field.clone())
                    }
                    Json::Number(number) if number.as_i64() == Some(0) => {
                        excludes.push(field.clone())
                    }
                    _ => {
                        return Err(DriverError::query(format!(
                            "projection for {field} must be 1, 0, true or false"
                        )));
                    }
                }
            }
            Ok(Some(json!({"includes": includes, "excludes": excludes})))
        }
        Json::Number(_) => Err(DriverError::query(
            "the projection must be a field list or an includes/excludes object",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_filters_match_everything() {
        assert_eq!(query_clause(None).unwrap(), json!({"match_all": {}}));
        assert_eq!(
            query_clause(Some(&json!({}))).unwrap(),
            json!({"match_all": {}})
        );
        let term = json!({"term": {"status": "paid"}});
        assert_eq!(query_clause(Some(&term)).unwrap(), term);
        assert_eq!(
            query_clause(Some(&json!({"query": term.clone()}))).unwrap(),
            term
        );
        assert!(query_clause(Some(&json!("status:paid"))).is_err());
    }

    #[test]
    fn sorts_always_end_in_a_tiebreak() {
        assert_eq!(
            sort_clause(None).unwrap(),
            vec![json!({"_score": "desc"}), json!({"_doc": "asc"})]
        );
        assert_eq!(
            sort_clause(Some(&json!({"price": -1}))).unwrap(),
            vec![json!({"price": "desc"}), json!({"_doc": "asc"})]
        );
        assert_eq!(
            sort_clause(Some(&json!([{"date": {"order": "asc"}}, "_doc"]))).unwrap(),
            vec![json!({"date": {"order": "asc"}}), json!("_doc")]
        );
        assert_eq!(sort_clause(Some(&json!("name"))).unwrap().len(), 2);
        assert!(sort_clause(Some(&json!({"price": "up"}))).is_err());
    }

    #[test]
    fn projections_become_source_filters() {
        assert_eq!(source_clause(None).unwrap(), None);
        assert_eq!(
            source_clause(Some(&json!(["a", "b.*"]))).unwrap(),
            Some(json!(["a", "b.*"]))
        );
        assert_eq!(
            source_clause(Some(&json!({"a": 1, "secret": 0}))).unwrap(),
            Some(json!({"includes": ["a"], "excludes": ["secret"]}))
        );
        let explicit = json!({"includes": ["a"]});
        assert_eq!(
            source_clause(Some(&explicit)).unwrap(),
            Some(explicit.clone())
        );
        assert!(source_clause(Some(&json!({"a": "yes"}))).is_err());
    }

    #[test]
    fn bodies_request_sequence_numbers_for_conditional_writes() {
        let body = search_body(&DocumentQuery::first_page(20), 20).unwrap();
        assert_eq!(body["size"], 20);
        assert_eq!(body["seq_no_primary_term"], true);
        assert!(!body.contains_key("_source"));
    }
}
