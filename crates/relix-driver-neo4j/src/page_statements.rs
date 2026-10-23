/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::cypher::quote;
use crate::graph_object::GraphObject;
use neo4rs::{Query, query};
use relix_core::TableQuery;

pub(crate) const PROPERTIES_ALIAS: &str = "properties";
const KEY_SAMPLE_SIZE: i64 = 1_000;

/// Builds the page query for a label (variable `n`) or relationship type (variable
/// `r` between start node `a` and end node `b`). The filter is used verbatim as the
/// `WHERE` condition on those variables. Without an explicit sort the rows follow the
/// store order Neo4j scans them in.
pub(crate) fn page_query(object: &GraphObject, table_query: &TableQuery) -> Query {
    let variable = object.entity.variable();
    let mut cypher = format!(
        "MATCH {}{}",
        object.pattern(),
        where_clause(table_query.filter.as_deref())
    );
    let projections: Vec<String> = object
        .entity
        .fixed_columns()
        .iter()
        .map(|(name, expression)| format!("{expression} AS {}", quote(name)))
        .collect();
    cypher.push_str(&format!(
        " RETURN {}, properties({variable}) AS {PROPERTIES_ALIAS}",
        projections.join(", ")
    ));
    if !table_query.sort.is_empty() {
        let order: Vec<String> = table_query
            .sort
            .iter()
            .map(|key| {
                format!(
                    "{} {}",
                    sort_expression(object, &key.column),
                    key.direction.sql_keyword()
                )
            })
            .collect();
        cypher.push_str(&format!(" ORDER BY {}", order.join(", ")));
    }
    cypher.push_str(" SKIP $skip LIMIT $limit");
    query(&cypher)
        .param(
            "skip",
            i64::try_from(table_query.offset).unwrap_or(i64::MAX),
        )
        .param("limit", i64::from(table_query.limit) + 1)
}

pub(crate) fn count_query(object: &GraphObject, filter: Option<&str>) -> Query {
    query(&format!(
        "MATCH {}{} RETURN count({}) AS count",
        object.pattern(),
        where_clause(filter),
        object.entity.variable()
    ))
}

pub(crate) fn key_sample_query(object: &GraphObject) -> Query {
    let variable = object.entity.variable();
    query(&format!(
        "MATCH {} WITH {variable} LIMIT $sample UNWIND keys({variable}) AS key \
         RETURN collect(DISTINCT key) AS keys",
        object.pattern()
    ))
    .param("sample", KEY_SAMPLE_SIZE)
}

fn sort_expression(object: &GraphObject, column: &str) -> String {
    object
        .entity
        .fixed_columns()
        .iter()
        .find(|(name, _)| *name == column)
        .map(|(_, expression)| (*expression).to_owned())
        .unwrap_or_else(|| format!("{}.{}", object.entity.variable(), quote(column)))
}

fn where_clause(filter: Option<&str>) -> String {
    let condition = filter
        .unwrap_or_default()
        .trim()
        .trim_end_matches(';')
        .trim();
    if condition.is_empty() {
        String::new()
    } else {
        format!(" WHERE {condition}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_object::GraphEntity;
    use pretty_assertions::assert_eq;
    use relix_core::{SortDirection, SortKey};

    fn person() -> GraphObject {
        GraphObject {
            entity: GraphEntity::Node,
            name: "Person".into(),
        }
    }

    #[test]
    fn node_pages_project_identity_labels_and_properties() {
        let table_query = TableQuery {
            filter: Some("n.age > 30;".into()),
            sort: vec![SortKey {
                column: "name".into(),
                direction: SortDirection::Descending,
            }],
            tiebreaker: vec!["elementId".into()],
            offset: 20,
            limit: 10,
        };
        let page = page_query(&person(), &table_query);
        assert_eq!(
            page.query(),
            "MATCH (n:`Person`) WHERE n.age > 30 RETURN elementId(n) AS `elementId`, \
             labels(n) AS `labels`, properties(n) AS properties ORDER BY n.`name` DESC \
             SKIP $skip LIMIT $limit"
        );
        assert!(page.has_param_key("skip") && page.has_param_key("limit"));
    }

    #[test]
    fn relationship_pages_include_their_endpoints() {
        let knows = GraphObject {
            entity: GraphEntity::Relationship,
            name: "KNOWS".into(),
        };
        let table_query = TableQuery {
            sort: vec![SortKey {
                column: "startElementId".into(),
                direction: SortDirection::Ascending,
            }],
            ..TableQuery::default()
        };
        assert_eq!(
            page_query(&knows, &table_query).query(),
            "MATCH (a)-[r:`KNOWS`]->(b) RETURN elementId(r) AS `elementId`, type(r) AS `type`, \
             elementId(a) AS `startElementId`, elementId(b) AS `endElementId`, \
             properties(r) AS properties ORDER BY elementId(a) ASC SKIP $skip LIMIT $limit"
        );
        assert_eq!(
            count_query(&knows, Some(" ")).query(),
            "MATCH (a)-[r:`KNOWS`]->(b) RETURN count(r) AS count"
        );
    }
}
