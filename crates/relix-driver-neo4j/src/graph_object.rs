/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::cypher::{fetch_all, quote};
use crate::row_reader::optional_text;
use neo4rs::{Graph, query};
use relix_core::{ObjectKind, TableRef};
use relix_driver_api::DriverError;

const NODE_NAMESPACE: &str = "nodes";
const RELATIONSHIP_NAMESPACE: &str = "relationships";
pub(crate) const ELEMENT_ID: &str = "elementId";
const LABELS_CYPHER: &str = "CALL db.labels() YIELD label RETURN label AS name";
const TYPES_CYPHER: &str =
    "CALL db.relationshipTypes() YIELD relationshipType RETURN relationshipType AS name";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphEntity {
    Node,
    Relationship,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphObject {
    pub(crate) entity: GraphEntity,
    pub(crate) name: String,
}

impl GraphEntity {
    pub(crate) fn kind(self) -> ObjectKind {
        match self {
            Self::Node => ObjectKind::NodeLabel,
            Self::Relationship => ObjectKind::RelationshipType,
        }
    }

    fn namespace(self) -> &'static str {
        match self {
            Self::Node => NODE_NAMESPACE,
            Self::Relationship => RELATIONSHIP_NAMESPACE,
        }
    }

    pub(crate) fn variable(self) -> &'static str {
        match self {
            Self::Node => "n",
            Self::Relationship => "r",
        }
    }

    pub(crate) fn schema_entity(self) -> &'static str {
        match self {
            Self::Node => "NODE",
            Self::Relationship => "RELATIONSHIP",
        }
    }

    pub(crate) fn match_by_element_id(self) -> &'static str {
        match self {
            Self::Node => "MATCH (n) WHERE elementId(n) = $id",
            Self::Relationship => "MATCH ()-[r]->() WHERE elementId(r) = $id",
        }
    }

    /// The fixed grid columns that precede the properties, with the Cypher expression
    /// each one is read from (`a` and `b` are a relationship's start and end node).
    pub(crate) fn fixed_columns(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Node => &[(ELEMENT_ID, "elementId(n)"), ("labels", "labels(n)")],
            Self::Relationship => &[
                (ELEMENT_ID, "elementId(r)"),
                ("type", "type(r)"),
                ("startElementId", "elementId(a)"),
                ("endElementId", "elementId(b)"),
            ],
        }
    }

    pub(crate) fn is_fixed_column(self, column: &str) -> bool {
        self.fixed_columns().iter().any(|(name, _)| *name == column)
    }
}

impl GraphObject {
    pub(crate) fn table_ref(database: &str, entity: GraphEntity, name: String) -> TableRef {
        TableRef::new(
            Some(database.to_owned()),
            Some(entity.namespace().to_owned()),
            name,
        )
    }

    /// Identifies the label or relationship type a table reference points to. References
    /// created by the explorer carry the entity in their schema part; others are looked
    /// up among the database's labels first and relationship types second.
    pub(crate) async fn resolve(
        graph: &Graph,
        database: &str,
        table: &TableRef,
    ) -> Result<Self, DriverError> {
        let name = table.name.clone();
        match table.schema.as_deref() {
            Some(NODE_NAMESPACE) => {
                return Ok(Self {
                    entity: GraphEntity::Node,
                    name,
                });
            }
            Some(RELATIONSHIP_NAMESPACE) => {
                return Ok(Self {
                    entity: GraphEntity::Relationship,
                    name,
                });
            }
            _ => {}
        }
        if labels(graph, database).await?.contains(&name) {
            return Ok(Self {
                entity: GraphEntity::Node,
                name,
            });
        }
        if relationship_types(graph, database).await?.contains(&name) {
            return Ok(Self {
                entity: GraphEntity::Relationship,
                name,
            });
        }
        Err(DriverError::query(format!(
            "'{name}' is neither a node label nor a relationship type in {database}"
        )))
    }

    pub(crate) fn pattern(&self) -> String {
        match self.entity {
            GraphEntity::Node => format!("(n:{})", quote(&self.name)),
            GraphEntity::Relationship => format!("(a)-[r:{}]->(b)", quote(&self.name)),
        }
    }
}

async fn names(graph: &Graph, database: &str, cypher: &str) -> Result<Vec<String>, DriverError> {
    let rows = fetch_all(graph, Some(database), query(cypher)).await?;
    let mut names = Vec::with_capacity(rows.len());
    for row in &rows {
        if let Some(name) = optional_text(row, "name")? {
            names.push(name);
        }
    }
    Ok(names)
}

pub(crate) async fn labels(graph: &Graph, database: &str) -> Result<Vec<String>, DriverError> {
    names(graph, database, LABELS_CYPHER).await
}

pub(crate) async fn relationship_types(
    graph: &Graph,
    database: &str,
) -> Result<Vec<String>, DriverError> {
    names(graph, database, TYPES_CYPHER).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_quote_names() {
        let label = GraphObject {
            entity: GraphEntity::Node,
            name: "My Label".into(),
        };
        assert_eq!(label.pattern(), "(n:`My Label`)");
        let relationship = GraphObject {
            entity: GraphEntity::Relationship,
            name: "KNOWS".into(),
        };
        assert_eq!(relationship.pattern(), "(a)-[r:`KNOWS`]->(b)");
        assert!(GraphEntity::Relationship.is_fixed_column("startElementId"));
        assert!(!GraphEntity::Node.is_fixed_column("name"));
    }

    #[test]
    fn table_refs_carry_the_entity() {
        let table = GraphObject::table_ref("neo4j", GraphEntity::Relationship, "KNOWS".into());
        assert_eq!(table.schema.as_deref(), Some("relationships"));
        assert_eq!(table.to_string(), "relationships.KNOWS");
    }
}
