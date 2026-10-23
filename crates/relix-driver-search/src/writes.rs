/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::SearchClient;
use crate::document_key::DocumentKey;
use crate::paths::segment;
use relix_core::DocumentWriteOutcome;
use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::{Method, StatusCode};
use serde_json::{Map, Value as Json};

const REFRESH: &str = "refresh=wait_for";

/// Indexes a new document. A top-level `_id` in the document is taken out of the body
/// and used with `_create`, which fails instead of overwriting an existing document.
pub async fn insert(
    client: &SearchClient,
    collection: &str,
    document: &Json,
) -> Result<DocumentWriteOutcome, DriverError> {
    let mut body = document_object(document)?;
    let target = segment(collection)?;
    let (method, path) = match body.remove("_id") {
        None | Some(Json::Null) => (Method::POST, format!("/{target}/_doc?{REFRESH}")),
        Some(id) => {
            let id = id_text(&id)?;
            let path = format!("/{target}/_create/{}?{REFRESH}", segment(&id)?);
            (Method::PUT, path)
        }
    };
    let response = client
        .json(method, &path, Some(&Json::Object(body)))
        .await?;
    Ok(outcome(&response))
}

/// Replaces a document's source. When the key carries `_seq_no` and `_primary_term`
/// the write is conditional, so a document changed by someone else since it was read
/// is reported as a concurrent modification instead of being overwritten.
pub async fn replace(
    client: &SearchClient,
    collection: &str,
    key: &Json,
    document: &Json,
) -> Result<DocumentWriteOutcome, DriverError> {
    let key = DocumentKey::parse(key)?;
    let mut body = document_object(document)?;
    if let Some(id) = body.remove("_id")
        && id_text(&id)? != key.id
    {
        return Err(DriverError::query(
            "the _id of a document cannot be changed",
        ));
    }
    let path = format!(
        "/{}/_doc/{}?{REFRESH}{}",
        segment(key.index_or(collection))?,
        segment(&key.id)?,
        key.concurrency_query()
    );
    let response = client
        .json(Method::PUT, &path, Some(&Json::Object(body)))
        .await?;
    Ok(outcome(&response))
}

pub async fn delete(
    client: &SearchClient,
    collection: &str,
    key: &Json,
) -> Result<DocumentWriteOutcome, DriverError> {
    let key = DocumentKey::parse(key)?;
    let path = format!(
        "/{}/_doc/{}?{REFRESH}{}",
        segment(key.index_or(collection))?,
        segment(&key.id)?,
        key.concurrency_query()
    );
    let response = client.call(Method::DELETE, &path, None).await?;
    if response.status == StatusCode::NOT_FOUND && response.body.contains("\"not_found\"") {
        return Err(DriverError::new(
            ErrorKind::ConcurrentModification,
            format!("document {} no longer exists", key.id),
        ));
    }
    Ok(outcome(&response.into_json()?))
}

fn document_object(document: &Json) -> Result<Map<String, Json>, DriverError> {
    match document {
        Json::Object(fields) => Ok(fields.clone()),
        _ => Err(DriverError::query("a document must be a JSON object")),
    }
}

fn id_text(id: &Json) -> Result<String, DriverError> {
    match id {
        Json::String(text) if !text.is_empty() => Ok(text.clone()),
        Json::Number(number) => Ok(number.to_string()),
        _ => Err(DriverError::query("_id must be a non-empty string")),
    }
}

pub fn outcome(response: &Json) -> DocumentWriteOutcome {
    let result = response["result"].as_str().unwrap_or("done");
    let affected = u64::from(matches!(result, "created" | "updated" | "deleted"));
    let key = DocumentKey::from_metadata(response);
    let message = match &key {
        Some(key) => format!(
            "{result} {}/{}",
            key.index.as_deref().unwrap_or("?"),
            key.id
        ),
        None => result.to_owned(),
    };
    DocumentWriteOutcome {
        affected,
        key: key.map(|key| key.to_json()),
        message: Some(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn write_responses_return_the_new_key() {
        let response = json!({"_index": "orders", "_id": "x1", "_version": 2, "result": "updated",
            "_seq_no": 5, "_primary_term": 1});
        let outcome = outcome(&response);
        assert_eq!(outcome.affected, 1);
        assert_eq!(
            outcome.key,
            Some(json!({"_index": "orders", "_id": "x1", "_seq_no": 5, "_primary_term": 1}))
        );
        assert_eq!(outcome.message.as_deref(), Some("updated orders/x1"));
        let noop = super::outcome(&json!({"_index": "o", "_id": "1", "result": "noop"}));
        assert_eq!(noop.affected, 0);
    }

    #[test]
    fn ids_must_be_strings_or_numbers() {
        assert_eq!(id_text(&json!("a")).unwrap(), "a");
        assert_eq!(id_text(&json!(12)).unwrap(), "12");
        assert!(id_text(&json!("")).is_err());
        assert!(id_text(&json!({"a": 1})).is_err());
        assert!(document_object(&json!([1])).is_err());
    }
}
