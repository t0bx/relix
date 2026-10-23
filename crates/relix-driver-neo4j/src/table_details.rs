/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::cypher::fetch_all;
use crate::graph_object::{ELEMENT_ID, GraphEntity, GraphObject};
use crate::row_reader::{optional_text, text_list};
use crate::schema_properties::observed_properties;
use neo4rs::{Graph, query};
use relix_core::{ColumnInfo, IndexInfo, TableDetails};
use relix_driver_api::DriverError;

const INDEXES_CYPHER: &str = "SHOW INDEXES \
     YIELD name, entityType, labelsOrTypes, properties, owningConstraint, createStatement \
     WHERE entityType = $entity AND $name IN labelsOrTypes \
     RETURN name, properties, owningConstraint, createStatement";
const CONSTRAINTS_CYPHER: &str = "SHOW CONSTRAINTS \
     YIELD name, entityType, labelsOrTypes, createStatement \
     WHERE entityType = $entity AND $name IN labelsOrTypes \
     RETURN name, createStatement";

pub(crate) async fn table_details(
    graph: &Graph,
    database: &str,
    object: &GraphObject,
) -> Result<TableDetails, DriverError> {
    let mut columns = fixed_columns(object.entity);
    for property in observed_properties(graph, database, object).await? {
        columns.push(ColumnInfo {
            name: property.name,
            data_type: property.types.join(" | "),
            nullable: !property.always_present,
            default_value: None,
            ordinal: u32::try_from(columns.len() + 1).unwrap_or(u32::MAX),
            is_primary_key: false,
            is_auto_increment: false,
            comment: Some("property".to_owned()),
        });
    }
    let (indexes, mut statements) = indexes(graph, database, object).await?;
    statements.extend(constraint_statements(graph, database, object).await?);
    Ok(TableDetails {
        table: GraphObject::table_ref(database, object.entity, object.name.clone()),
        kind: object.entity.kind(),
        columns,
        primary_key: vec![ELEMENT_ID.to_owned()],
        indexes,
        foreign_keys: Vec::new(),
        definition: (!statements.is_empty()).then(|| statements.join(";\n") + ";"),
    })
}

pub(crate) async fn definition(
    graph: &Graph,
    database: &str,
    object: &GraphObject,
) -> Result<Option<String>, DriverError> {
    Ok(table_details(graph, database, object).await?.definition)
}

fn fixed_columns(entity: GraphEntity) -> Vec<ColumnInfo> {
    entity
        .fixed_columns()
        .iter()
        .enumerate()
        .map(|(index, (name, expression))| ColumnInfo {
            name: (*name).to_owned(),
            data_type: if *name == "labels" {
                "List<String>"
            } else {
                "String"
            }
            .to_owned(),
            nullable: false,
            default_value: None,
            ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
            is_primary_key: *name == ELEMENT_ID,
            is_auto_increment: false,
            comment: Some((*expression).to_owned()),
        })
        .collect()
}

/// Returns the indexes on the label or type, plus the `CREATE` statements of the
/// indexes that are not owned by a constraint (those appear with their constraint).
async fn indexes(
    graph: &Graph,
    database: &str,
    object: &GraphObject,
) -> Result<(Vec<IndexInfo>, Vec<String>), DriverError> {
    let cypher = query(INDEXES_CYPHER)
        .param("entity", object.entity.schema_entity())
        .param("name", object.name.as_str());
    let rows = fetch_all(graph, Some(database), cypher).await?;
    let mut indexes = Vec::with_capacity(rows.len());
    let mut statements = Vec::new();
    for row in &rows {
        let owned = optional_text(row, "owningConstraint")?.is_some();
        if !owned && let Some(statement) = optional_text(row, "createStatement")? {
            statements.push(statement);
        }
        indexes.push(IndexInfo {
            name: optional_text(row, "name")?.unwrap_or_default(),
            columns: text_list(row, "properties")?,
            is_unique: owned,
            is_primary: false,
        });
    }
    Ok((indexes, statements))
}

async fn constraint_statements(
    graph: &Graph,
    database: &str,
    object: &GraphObject,
) -> Result<Vec<String>, DriverError> {
    let cypher = query(CONSTRAINTS_CYPHER)
        .param("entity", object.entity.schema_entity())
        .param("name", object.name.as_str());
    let rows = fetch_all(graph, Some(database), cypher).await?;
    let mut statements = Vec::with_capacity(rows.len());
    for row in &rows {
        if let Some(statement) = optional_text(row, "createStatement")? {
            statements.push(statement);
        }
    }
    Ok(statements)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_id_identifies_rows() {
        let columns = fixed_columns(GraphEntity::Relationship);
        let names: Vec<&str> = columns.iter().map(|column| column.name.as_str()).collect();
        assert_eq!(
            names,
            ["elementId", "type", "startElementId", "endElementId"]
        );
        assert!(columns[0].is_primary_key);
        assert!(!columns[1].is_primary_key);
    }
}
