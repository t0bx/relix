/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::cypher::{fetch_all, quote};
use crate::graph_object::{GraphEntity, GraphObject};
use crate::row_reader::{optional_bool, optional_text, text_list};
use neo4rs::{Graph, query};
use relix_driver_api::DriverError;
use std::collections::{BTreeMap, BTreeSet};

const NODE_PROPERTIES_CYPHER: &str = "CALL db.schema.nodeTypeProperties() \
     YIELD nodeType, nodeLabels, propertyName, propertyTypes, mandatory \
     WHERE $name IN nodeLabels \
     RETURN nodeType AS shape, propertyName, propertyTypes, mandatory";
const RELATIONSHIP_PROPERTIES_CYPHER: &str = "CALL db.schema.relTypeProperties() \
     YIELD relType, propertyName, propertyTypes, mandatory \
     WHERE relType = $name \
     RETURN relType AS shape, propertyName, propertyTypes, mandatory";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedProperty {
    pub(crate) name: String,
    pub(crate) types: Vec<String>,
    pub(crate) always_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShapeProperty {
    shape: String,
    name: Option<String>,
    types: Vec<String>,
    mandatory: bool,
}

/// Reads the property keys and value types Neo4j observed for a label or relationship
/// type. A node label can occur in several label combinations ("shapes"); a property
/// is only reported as always present when every shape carries it on every member.
pub(crate) async fn observed_properties(
    graph: &Graph,
    database: &str,
    object: &GraphObject,
) -> Result<Vec<ObservedProperty>, DriverError> {
    let cypher = match object.entity {
        GraphEntity::Node => query(NODE_PROPERTIES_CYPHER).param("name", object.name.as_str()),
        GraphEntity::Relationship => {
            query(RELATIONSHIP_PROPERTIES_CYPHER).param("name", format!(":{}", quote(&object.name)))
        }
    };
    let rows = fetch_all(graph, Some(database), cypher).await?;
    let mut shapes = Vec::with_capacity(rows.len());
    for row in &rows {
        shapes.push(ShapeProperty {
            shape: optional_text(row, "shape")?.unwrap_or_default(),
            name: optional_text(row, "propertyName")?,
            types: text_list(row, "propertyTypes")?,
            mandatory: optional_bool(row, "mandatory")?.unwrap_or(false),
        });
    }
    Ok(merge(shapes))
}

fn merge(rows: Vec<ShapeProperty>) -> Vec<ObservedProperty> {
    let shapes: BTreeSet<&str> = rows.iter().map(|row| row.shape.as_str()).collect();
    let mut properties: BTreeMap<&str, (BTreeSet<&str>, BTreeSet<&str>, bool)> = BTreeMap::new();
    for row in &rows {
        let Some(name) = row.name.as_deref() else {
            continue;
        };
        let entry = properties
            .entry(name)
            .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), true));
        entry.0.extend(row.types.iter().map(String::as_str));
        entry.1.insert(row.shape.as_str());
        entry.2 &= row.mandatory;
    }
    properties
        .into_iter()
        .map(|(name, (types, present_in, mandatory))| ObservedProperty {
            name: name.to_owned(),
            types: types.into_iter().map(str::to_owned).collect(),
            always_present: mandatory && present_in.len() == shapes.len(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(shape: &str, name: Option<&str>, types: &[&str], mandatory: bool) -> ShapeProperty {
        ShapeProperty {
            shape: shape.into(),
            name: name.map(str::to_owned),
            types: types.iter().map(|value| (*value).to_owned()).collect(),
            mandatory,
        }
    }

    #[test]
    fn properties_are_merged_across_label_combinations() {
        let merged = merge(vec![
            row(":`Person`", Some("name"), &["String"], true),
            row(":`Person`", Some("age"), &["Long"], true),
            row(":`Admin`:`Person`", Some("name"), &["String"], true),
            row(":`Admin`:`Person`", Some("age"), &["String"], false),
            row(":`Admin`:`Person`", Some("tags"), &["StringArray"], true),
            row(":`Guest`:`Person`", None, &[], false),
        ]);
        let summary: Vec<(&str, Vec<&str>, bool)> = merged
            .iter()
            .map(|property| {
                (
                    property.name.as_str(),
                    property.types.iter().map(String::as_str).collect(),
                    property.always_present,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("age", vec!["Long", "String"], false),
                ("name", vec!["String"], false),
                ("tags", vec!["StringArray"], false),
            ]
        );
    }
}
