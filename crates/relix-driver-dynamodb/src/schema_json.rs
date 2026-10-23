/*
 * @author t0bx
 * @since 23.10.2026
 */

use aws_sdk_dynamodb::types::{
    AttributeDefinition, KeySchemaElement, Projection, ProvisionedThroughputDescription,
};
use serde_json::{Value as Json, json};

pub fn key_schema_json(elements: &[KeySchemaElement]) -> Json {
    Json::Array(
        elements
            .iter()
            .map(|element| {
                json!({
                    "AttributeName": element.attribute_name(),
                    "KeyType": element.key_type().as_str(),
                })
            })
            .collect(),
    )
}

pub fn attribute_definitions_json(definitions: &[AttributeDefinition]) -> Json {
    Json::Array(
        definitions
            .iter()
            .map(|definition| {
                json!({
                    "AttributeName": definition.attribute_name(),
                    "AttributeType": definition.attribute_type().as_str(),
                })
            })
            .collect(),
    )
}

pub fn projection_json(projection: Option<&Projection>) -> Json {
    let Some(projection) = projection else {
        return Json::Null;
    };
    let mut value = json!({
        "ProjectionType": projection.projection_type().map(|kind| kind.as_str()),
    });
    if !projection.non_key_attributes().is_empty() {
        value["NonKeyAttributes"] = json!(projection.non_key_attributes());
    }
    value
}

pub fn projection_label(projection: Option<&Projection>) -> String {
    projection
        .and_then(Projection::projection_type)
        .map(|kind| kind.as_str().to_owned())
        .unwrap_or_else(|| "ALL".to_owned())
}

pub fn throughput_json(throughput: Option<&ProvisionedThroughputDescription>) -> Option<Json> {
    let throughput = throughput?;
    let read = throughput.read_capacity_units.unwrap_or(0);
    let write = throughput.write_capacity_units.unwrap_or(0);
    (read > 0 || write > 0).then(|| {
        json!({
            "ReadCapacityUnits": read,
            "WriteCapacityUnits": write,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::types::{KeyType, ProjectionType};

    #[test]
    fn key_schemas_and_projections_use_the_aws_shape() {
        let element = KeySchemaElement::builder()
            .attribute_name("pk")
            .key_type(KeyType::Hash)
            .build()
            .unwrap();
        assert_eq!(
            key_schema_json(&[element]),
            json!([{"AttributeName": "pk", "KeyType": "HASH"}])
        );
        let projection = Projection::builder()
            .projection_type(ProjectionType::Include)
            .non_key_attributes("name")
            .build();
        assert_eq!(
            projection_json(Some(&projection)),
            json!({"ProjectionType": "INCLUDE", "NonKeyAttributes": ["name"]})
        );
        assert_eq!(projection_label(Some(&projection)), "INCLUDE");
    }

    #[test]
    fn on_demand_tables_have_no_throughput() {
        let on_demand = ProvisionedThroughputDescription::builder()
            .read_capacity_units(0)
            .write_capacity_units(0)
            .build();
        assert_eq!(throughput_json(Some(&on_demand)), None);
        let provisioned = ProvisionedThroughputDescription::builder()
            .read_capacity_units(5)
            .write_capacity_units(2)
            .build();
        assert_eq!(
            throughput_json(Some(&provisioned)),
            Some(json!({"ReadCapacityUnits": 5, "WriteCapacityUnits": 2}))
        );
    }
}
