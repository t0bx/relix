/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::SearchClient;
use crate::paths::concrete_name;
use crate::resolve::resolve_target;
use relix_core::{CollectionKind, CollectionRef, DocumentWriteOutcome};
use relix_driver_api::DriverError;
use relix_driver_http::Method;
use serde_json::Value as Json;

/// Creates an index from `{"name": ..., "mappings": ..., "settings": ..., "aliases": ...}`;
/// everything except `name` is sent as the body of `PUT /<name>`.
pub async fn create_index(
    client: &SearchClient,
    definition: &Json,
) -> Result<DocumentWriteOutcome, DriverError> {
    let Json::Object(fields) = definition else {
        return Err(definition_error());
    };
    let mut body = fields.clone();
    let name = match body.remove("name") {
        Some(Json::String(name)) if !name.trim().is_empty() => name.trim().to_owned(),
        _ => return Err(definition_error()),
    };
    client
        .json(
            Method::PUT,
            &format!("/{}", concrete_name(&name)?),
            Some(&Json::Object(body)),
        )
        .await?;
    Ok(admin_outcome(format!("index {name} created")))
}

/// Deletes the collection itself (an index, or a data stream with its backing indices)
/// or one concrete index behind it. Aliases hold no data, so dropping an alias by name
/// is refused; one of its indices has to be named instead.
pub async fn drop_index(
    client: &SearchClient,
    collection: &CollectionRef,
    name: &str,
) -> Result<DocumentWriteOutcome, DriverError> {
    let target = resolve_target(client, &collection.name).await?;
    let encoded = concrete_name(name)?;
    let path = if name == collection.name {
        match target.kind {
            CollectionKind::DataStream => format!("/_data_stream/{encoded}"),
            CollectionKind::Alias => {
                return Err(DriverError::unsupported(format!(
                    "{name} is an alias; drop one of its indices instead: {}",
                    target.members.join(", ")
                )));
            }
            _ => format!("/{encoded}"),
        }
    } else if target.members.iter().any(|member| member == name) {
        format!("/{encoded}")
    } else {
        return Err(DriverError::query(format!(
            "index {name} does not belong to {}",
            collection.name
        )));
    };
    client.json(Method::DELETE, &path, None).await?;
    let label = match (name == collection.name, target.kind) {
        (true, CollectionKind::DataStream) => "data stream",
        _ => "index",
    };
    Ok(admin_outcome(format!("{label} {name} deleted")))
}

fn admin_outcome(message: String) -> DocumentWriteOutcome {
    DocumentWriteOutcome {
        affected: 1,
        key: None,
        message: Some(message),
    }
}

fn definition_error() -> DriverError {
    DriverError::query(
        "an index definition needs a name: {\"name\": \"orders\", \"mappings\": {...}, \"settings\": {...}}",
    )
}
