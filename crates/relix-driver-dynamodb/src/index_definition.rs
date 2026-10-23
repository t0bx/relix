/*
 * @author t0bx
 * @since 23.10.2026
 */

use aws_sdk_dynamodb::types::{
    AttributeDefinition, CreateGlobalSecondaryIndexAction, KeySchemaElement, KeyType, Projection,
    ProjectionType, ProvisionedThroughput, ScalarAttributeType,
};
use relix_driver_api::DriverError;
use serde_json::{Map, Value as Json};

#[derive(Debug, Clone, PartialEq)]
pub struct GlobalIndexDefinition {
    pub index: CreateGlobalSecondaryIndexAction,
    pub attribute_definitions: Vec<AttributeDefinition>,
}

/// Parses a global secondary index definition in the shape of the UpdateTable API:
/// `{"IndexName": "byEmail", "KeySchema": [{"AttributeName": "email", "KeyType": "HASH"}],
/// "AttributeDefinitions": [{"AttributeName": "email", "AttributeType": "S"}],
/// "Projection": {"ProjectionType": "ALL"}, "ProvisionedThroughput": {...}}`.
/// The projection defaults to `ALL`. Key attributes missing from `AttributeDefinitions`
/// are taken from the table's definitions; provisioned tables reuse the table's
/// throughput when none is given.
pub fn global_index_definition(
    definition: &Json,
    table_definitions: &[AttributeDefinition],
    table_throughput: Option<ProvisionedThroughput>,
) -> Result<GlobalIndexDefinition, DriverError> {
    let Json::Object(fields) = definition else {
        return Err(invalid("the definition must be an object"));
    };
    let name = fields
        .get("IndexName")
        .and_then(Json::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| invalid("IndexName is required"))?;
    let key_schema = key_schema(fields.get("KeySchema"))?;
    let declared = attribute_definitions(fields.get("AttributeDefinitions"))?;
    let mut definitions = Vec::new();
    for element in &key_schema {
        let attribute = element.attribute_name();
        let definition = declared
            .iter()
            .chain(table_definitions)
            .find(|definition| definition.attribute_name() == attribute)
            .ok_or_else(|| {
                invalid(&format!(
                    "AttributeDefinitions needs the type of key attribute \"{attribute}\""
                ))
            })?;
        definitions.push(definition.clone());
    }
    let throughput = match fields.get("ProvisionedThroughput") {
        Some(value) => Some(throughput(value)?),
        None => table_throughput,
    };
    let index = CreateGlobalSecondaryIndexAction::builder()
        .index_name(name)
        .set_key_schema(Some(key_schema))
        .projection(projection(fields.get("Projection"))?)
        .set_provisioned_throughput(throughput)
        .build()
        .map_err(|error| invalid(&error.to_string()))?;
    Ok(GlobalIndexDefinition {
        index,
        attribute_definitions: definitions,
    })
}

fn key_schema(value: Option<&Json>) -> Result<Vec<KeySchemaElement>, DriverError> {
    let Some(Json::Array(elements)) = value else {
        return Err(invalid(
            "KeySchema must list a HASH and an optional RANGE key",
        ));
    };
    let schema = elements
        .iter()
        .map(|element| {
            let element = object(element, "KeySchema entries")?;
            let kind = match string(element, "KeyType")? {
                "HASH" => KeyType::Hash,
                "RANGE" => KeyType::Range,
                other => return Err(invalid(&format!("unknown KeyType \"{other}\""))),
            };
            KeySchemaElement::builder()
                .attribute_name(string(element, "AttributeName")?)
                .key_type(kind)
                .build()
                .map_err(|error| invalid(&error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let hashes = schema
        .iter()
        .filter(|element| *element.key_type() == KeyType::Hash)
        .count();
    if hashes != 1 || schema.len() > 2 {
        return Err(invalid(
            "KeySchema needs exactly one HASH and at most one RANGE key",
        ));
    }
    Ok(schema)
}

fn attribute_definitions(value: Option<&Json>) -> Result<Vec<AttributeDefinition>, DriverError> {
    let elements = match value {
        None | Some(Json::Null) => return Ok(Vec::new()),
        Some(Json::Array(elements)) => elements,
        Some(_) => return Err(invalid("AttributeDefinitions must be a list")),
    };
    elements
        .iter()
        .map(|element| {
            let element = object(element, "AttributeDefinitions entries")?;
            let kind = match string(element, "AttributeType")? {
                "S" => ScalarAttributeType::S,
                "N" => ScalarAttributeType::N,
                "B" => ScalarAttributeType::B,
                other => return Err(invalid(&format!("unknown AttributeType \"{other}\""))),
            };
            AttributeDefinition::builder()
                .attribute_name(string(element, "AttributeName")?)
                .attribute_type(kind)
                .build()
                .map_err(|error| invalid(&error.to_string()))
        })
        .collect()
}

fn projection(value: Option<&Json>) -> Result<Projection, DriverError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(Projection::builder()
            .projection_type(ProjectionType::All)
            .build());
    };
    let fields = object(value, "Projection")?;
    let kind = match string(fields, "ProjectionType")? {
        "ALL" => ProjectionType::All,
        "KEYS_ONLY" => ProjectionType::KeysOnly,
        "INCLUDE" => ProjectionType::Include,
        other => return Err(invalid(&format!("unknown ProjectionType \"{other}\""))),
    };
    let attributes = match fields.get("NonKeyAttributes") {
        None => None,
        Some(Json::Array(names)) => Some(
            names
                .iter()
                .map(|name| {
                    name.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| invalid("NonKeyAttributes must be strings"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Some(_) => return Err(invalid("NonKeyAttributes must be a list")),
    };
    Ok(Projection::builder()
        .projection_type(kind)
        .set_non_key_attributes(attributes)
        .build())
}

fn throughput(value: &Json) -> Result<ProvisionedThroughput, DriverError> {
    let fields = object(value, "ProvisionedThroughput")?;
    let units = |name: &str| {
        fields
            .get(name)
            .and_then(Json::as_i64)
            .filter(|units| *units > 0)
            .ok_or_else(|| invalid(&format!("{name} must be a positive number")))
    };
    ProvisionedThroughput::builder()
        .read_capacity_units(units("ReadCapacityUnits")?)
        .write_capacity_units(units("WriteCapacityUnits")?)
        .build()
        .map_err(|error| invalid(&error.to_string()))
}

fn object<'a>(value: &'a Json, name: &str) -> Result<&'a Map<String, Json>, DriverError> {
    value
        .as_object()
        .ok_or_else(|| invalid(&format!("{name} must be an object")))
}

fn string<'a>(fields: &'a Map<String, Json>, name: &str) -> Result<&'a str, DriverError> {
    fields
        .get(name)
        .and_then(Json::as_str)
        .ok_or_else(|| invalid(&format!("{name} is required")))
}

fn invalid(reason: &str) -> DriverError {
    DriverError::query(format!(
        "invalid global secondary index definition: {reason}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn table_definition(name: &str) -> AttributeDefinition {
        AttributeDefinition::builder()
            .attribute_name(name)
            .attribute_type(ScalarAttributeType::S)
            .build()
            .unwrap()
    }

    #[test]
    fn definitions_with_defaults_are_completed() {
        let parsed = global_index_definition(
            &json!({
                "IndexName": "byEmail",
                "KeySchema": [
                    {"AttributeName": "email", "KeyType": "HASH"},
                    {"AttributeName": "pk", "KeyType": "RANGE"}
                ],
                "AttributeDefinitions": [{"AttributeName": "email", "AttributeType": "S"}]
            }),
            &[table_definition("pk")],
            None,
        )
        .unwrap();
        assert_eq!(parsed.index.index_name(), "byEmail");
        assert_eq!(
            parsed
                .index
                .projection()
                .and_then(Projection::projection_type),
            Some(&ProjectionType::All)
        );
        let names: Vec<_> = parsed
            .attribute_definitions
            .iter()
            .map(AttributeDefinition::attribute_name)
            .collect();
        assert_eq!(names, ["email", "pk"]);
    }

    #[test]
    fn include_projections_and_throughput_are_read() {
        let parsed = global_index_definition(
            &json!({
                "IndexName": "byCity",
                "KeySchema": [{"AttributeName": "city", "KeyType": "HASH"}],
                "AttributeDefinitions": [{"AttributeName": "city", "AttributeType": "S"}],
                "Projection": {"ProjectionType": "INCLUDE", "NonKeyAttributes": ["name"]},
                "ProvisionedThroughput": {"ReadCapacityUnits": 2, "WriteCapacityUnits": 1}
            }),
            &[],
            None,
        )
        .unwrap();
        let projection = parsed.index.projection().unwrap();
        assert_eq!(projection.non_key_attributes(), ["name"]);
        assert_eq!(
            parsed
                .index
                .provisioned_throughput()
                .map(ProvisionedThroughput::read_capacity_units),
            Some(2)
        );
    }

    #[test]
    fn incomplete_definitions_are_rejected() {
        let missing_type = global_index_definition(
            &json!({"IndexName": "x", "KeySchema": [{"AttributeName": "a", "KeyType": "HASH"}]}),
            &[],
            None,
        );
        assert!(missing_type.unwrap_err().message.contains("\"a\""));
        let two_hashes = global_index_definition(
            &json!({"IndexName": "x", "KeySchema": [
                {"AttributeName": "a", "KeyType": "HASH"},
                {"AttributeName": "b", "KeyType": "HASH"}
            ]}),
            &[table_definition("a"), table_definition("b")],
            None,
        );
        assert!(two_hashes.is_err());
        assert!(global_index_definition(&json!({"KeySchema": []}), &[], None).is_err());
    }
}
