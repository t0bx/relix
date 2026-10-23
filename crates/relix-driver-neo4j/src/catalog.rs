/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::cypher::{fetch_all, quote};
use crate::graph_object::{GraphEntity, GraphObject, labels, relationship_types};
use crate::row_reader::{optional_integer, optional_text};
use neo4rs::{Graph, Query, query};
use relix_core::{DatabaseInfo, ObjectInfo};
use relix_driver_api::DriverError;
use std::collections::BTreeMap;

const DATABASES_CYPHER: &str = "SHOW DATABASES YIELD name RETURN DISTINCT name";
const SYSTEM_DATABASE: &str = "system";
const COUNT_CHUNK: usize = 50;

/// Lists the databases with `SHOW DATABASES`. Users without the privilege to list
/// databases (and servers that refuse the command) still see their default database.
pub(crate) async fn databases(
    graph: &Graph,
    default_database: &str,
) -> Result<Vec<DatabaseInfo>, DriverError> {
    let mut names = match fetch_all(graph, None, query(DATABASES_CYPHER)).await {
        Ok(rows) => {
            let mut names = Vec::with_capacity(rows.len());
            for row in &rows {
                if let Some(name) = optional_text(row, "name")? {
                    names.push(name);
                }
            }
            names
        }
        Err(_) => Vec::new(),
    };
    if !names.iter().any(|name| name == default_database) {
        names.push(default_database.to_owned());
    }
    names.sort_by(|left, right| {
        (left == SYSTEM_DATABASE)
            .cmp(&(right == SYSTEM_DATABASE))
            .then_with(|| left.cmp(right))
    });
    names.dedup();
    Ok(names
        .into_iter()
        .map(|name| DatabaseInfo {
            is_default: name == default_database,
            name,
        })
        .collect())
}

pub(crate) async fn objects(graph: &Graph, database: &str) -> Result<Vec<ObjectInfo>, DriverError> {
    let mut objects = Vec::new();
    for (entity, names) in [
        (GraphEntity::Node, labels(graph, database).await?),
        (
            GraphEntity::Relationship,
            relationship_types(graph, database).await?,
        ),
    ] {
        let counts = counts(graph, database, entity, &names).await?;
        objects.extend(names.into_iter().map(|name| {
            let count = counts.get(&name).copied();
            ObjectInfo {
                object: GraphObject::table_ref(database, entity, name),
                kind: entity.kind(),
                comment: count.map(|count| count_comment(entity, count)),
            }
        }));
    }
    Ok(objects)
}

/// Counts the members of every label or relationship type. Each branch matches a single
/// label or type without a filter, which Neo4j answers from its count store; branches
/// are combined with `UNION ALL` in chunks to keep round trips low.
async fn counts(
    graph: &Graph,
    database: &str,
    entity: GraphEntity,
    names: &[String],
) -> Result<BTreeMap<String, i64>, DriverError> {
    let mut counts = BTreeMap::new();
    for chunk in names.chunks(COUNT_CHUNK) {
        let rows = fetch_all(graph, Some(database), count_query(entity, chunk)).await?;
        for row in &rows {
            if let (Some(name), Some(count)) =
                (optional_text(row, "name")?, optional_integer(row, "count")?)
            {
                counts.insert(name, count);
            }
        }
    }
    Ok(counts)
}

fn count_query(entity: GraphEntity, names: &[String]) -> Query {
    let branches: Vec<String> = names
        .iter()
        .enumerate()
        .map(|(index, name)| match entity {
            GraphEntity::Node => format!(
                "MATCH (n:{}) RETURN $name{index} AS name, count(n) AS count",
                quote(name)
            ),
            GraphEntity::Relationship => format!(
                "MATCH ()-[r:{}]->() RETURN $name{index} AS name, count(r) AS count",
                quote(name)
            ),
        })
        .collect();
    names.iter().enumerate().fold(
        query(&branches.join(" UNION ALL ")),
        |cypher, (index, name)| cypher.param(&format!("name{index}"), name.as_str()),
    )
}

fn count_comment(entity: GraphEntity, count: i64) -> String {
    let noun = match (entity, count) {
        (GraphEntity::Node, 1) => "node",
        (GraphEntity::Node, _) => "nodes",
        (GraphEntity::Relationship, 1) => "relationship",
        (GraphEntity::Relationship, _) => "relationships",
    };
    format!("{count} {noun}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_described_with_their_entity() {
        assert_eq!(count_comment(GraphEntity::Node, 1), "1 node");
        assert_eq!(
            count_comment(GraphEntity::Relationship, 3),
            "3 relationships"
        );
    }

    #[test]
    fn count_queries_union_one_branch_per_name() {
        let names = vec!["Person".to_owned(), "Odd`Name".to_owned()];
        let cypher = count_query(GraphEntity::Node, &names);
        assert_eq!(
            cypher.query(),
            "MATCH (n:`Person`) RETURN $name0 AS name, count(n) AS count UNION ALL \
             MATCH (n:`Odd``Name`) RETURN $name1 AS name, count(n) AS count"
        );
        assert!(cypher.has_param_key("name1"));
    }
}
