/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::MongoConnection;
use crate::errors::map_error;
use crate::extended_json::{
    document_to_editable_json, from_json, optional_document, to_editable_json,
};
use crate::key_spec::ordered_keys;
use futures_util::TryStreamExt;
use mongodb::Collection;
use mongodb::bson::Document;
use relix_core::{CollectionRef, DocumentPage, DocumentQuery};
use relix_driver_api::DriverError;
use serde_json::Value as Json;
use std::time::Duration;

pub const DEFAULT_PAGE_SIZE: u32 = 100;
const COUNT_BUDGET: Duration = Duration::from_secs(2);

impl MongoConnection {
    /// Reads one page of documents. The page token is the number of documents to skip;
    /// one extra document is requested to know whether another page exists. The total is
    /// only computed when it is cheap: collection metadata for an unfiltered collection,
    /// otherwise `countDocuments` bounded to two seconds.
    pub(crate) async fn find_page(
        &self,
        collection: &CollectionRef,
        query: &DocumentQuery,
    ) -> Result<DocumentPage, DriverError> {
        let handle = self.collection(collection)?;
        let filter = optional_document(query.filter.as_ref(), "filter")?;
        let skip = parse_page_token(query.page_token.as_deref())?;
        let limit = if query.limit == 0 {
            DEFAULT_PAGE_SIZE
        } else {
            query.limit
        };
        let mut find = handle
            .find(filter.clone())
            .sort(ordered_keys(query.sort.as_ref(), "sort")?)
            .projection(optional_document(query.projection.as_ref(), "projection")?)
            .skip(skip)
            .limit(i64::from(limit) + 1);
        if let Some(timeout) = self.query_timeout {
            find = find.max_time(timeout);
        }
        let mut documents: Vec<Document> = find
            .await
            .map_err(map_error)?
            .try_collect()
            .await
            .map_err(map_error)?;
        let has_more = documents.len() > limit as usize;
        documents.truncate(limit as usize);
        let next_page = has_more.then(|| (skip + u64::from(limit)).to_string());
        Ok(DocumentPage {
            documents: documents.into_iter().map(page_document).collect(),
            next_page,
            total: cheap_total(&handle, filter).await,
        })
    }
}

async fn cheap_total(handle: &Collection<Document>, filter: Document) -> Option<u64> {
    if filter.is_empty()
        && let Ok(count) = handle.estimated_document_count().await
    {
        return Some(count);
    }
    handle
        .count_documents(filter)
        .max_time(COUNT_BUDGET)
        .await
        .ok()
}

fn page_document(document: Document) -> relix_core::Document {
    let key = document
        .get("_id")
        .cloned()
        .map(to_editable_json)
        .unwrap_or(Json::Null);
    relix_core::Document {
        key,
        body: document_to_editable_json(document),
    }
}

pub fn parse_page_token(token: Option<&str>) -> Result<u64, DriverError> {
    match token.map(str::trim).filter(|token| !token.is_empty()) {
        None => Ok(0),
        Some(token) => token
            .parse()
            .map_err(|_| DriverError::query(format!("invalid page token \"{token}\""))),
    }
}

pub fn key_filter(key: &Json) -> Result<Document, DriverError> {
    let mut filter = Document::new();
    filter.insert("_id", from_json(key)?);
    Ok(filter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;
    use mongodb::bson::oid::ObjectId;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn page_tokens_are_skip_offsets() {
        assert_eq!(parse_page_token(None).unwrap(), 0);
        assert_eq!(parse_page_token(Some("150")).unwrap(), 150);
        assert!(parse_page_token(Some("abc")).is_err());
    }

    #[test]
    fn documents_are_keyed_by_their_id() {
        let id = ObjectId::parse_str("65f0a1b2c3d4e5f601234567").unwrap();
        let document = page_document(doc! { "_id": id, "n": 1 });
        assert_eq!(document.key, json!({"$oid": "65f0a1b2c3d4e5f601234567"}));
        assert_eq!(key_filter(&document.key).unwrap(), doc! { "_id": id });
        assert_eq!(page_document(doc! { "n": 1 }).key, Json::Null);
    }
}
