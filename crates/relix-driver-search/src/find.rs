/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::SearchClient;
use crate::document_key::DocumentKey;
use crate::hits::{SearchHits, hit_body};
use crate::page_token::PageToken;
use crate::paths::segment;
use crate::result_window::ResultWindows;
use crate::search_body::search_body;
use relix_core::{CollectionRef, Document, DocumentPage, DocumentQuery};
use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::Method;
use serde_json::{Value as Json, json};

const MAX_PAGE_SIZE: u64 = 10_000;
const DEFAULT_PAGE_SIZE: u64 = 100;

/// Runs one page of `POST /<collection>/_search`. The first pages use `from`/`size`;
/// once the next page would cross `index.max_result_window` the token switches to
/// `search_after` with the sort values (always ending in a `_doc` tiebreak) of the last
/// hit. The total is only reported when the server counted it exactly.
pub async fn find(
    client: &SearchClient,
    windows: &ResultWindows,
    exact_totals: bool,
    collection: &CollectionRef,
    query: &DocumentQuery,
) -> Result<DocumentPage, DriverError> {
    let window = windows.get(client, &collection.name).await?;
    let token = PageToken::decode(query.page_token.as_deref())?;
    let limit = match token {
        PageToken::Offset { from } => {
            page_size(query.limit).min(window.saturating_sub(from).max(1))
        }
        PageToken::After { .. } => page_size(query.limit),
    };
    let mut body = search_body(query, limit)?;
    match &token {
        PageToken::Offset { from } => {
            body.insert("from".to_owned(), Json::from(*from));
        }
        PageToken::After { values, .. } => {
            body.insert("search_after".to_owned(), Json::Array(values.clone()));
        }
    }
    if exact_totals {
        body.insert("track_total_hits".to_owned(), json!(true));
    }
    let path = format!("/{}/_search", segment(&collection.name)?);
    let response = client
        .json(Method::POST, &path, Some(&Json::Object(body)))
        .await?;
    let hits = SearchHits::from_response(&response).ok_or_else(|| {
        DriverError::new(ErrorKind::Protocol, "the search response contained no hits")
    })?;
    let documents: Vec<Document> = hits.hits.iter().filter_map(document_from_hit).collect();
    let next_from = token.from() + hits.hits.len() as u64;
    let next_page = if hits.hits.len() as u64 == limit && hits.total.exceeds(next_from) {
        PageToken::next(next_from, limit, window, hits.last_sort()).map(|next| next.encode())
    } else {
        None
    };
    Ok(DocumentPage {
        documents,
        next_page,
        total: hits.total.exact(),
    })
}

pub fn document_from_hit(hit: &Json) -> Option<Document> {
    let key = DocumentKey::from_metadata(hit)?;
    Some(Document {
        key: key.to_json(),
        body: hit_body(hit),
    })
}

fn page_size(limit: u32) -> u64 {
    match u64::from(limit) {
        0 => DEFAULT_PAGE_SIZE,
        limit => limit.min(MAX_PAGE_SIZE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_become_documents_keyed_by_index_and_id() {
        let hit = json!({"_index": "orders", "_id": "1", "_score": null, "_seq_no": 0,
            "_primary_term": 1, "_source": {"total": 9.5}, "sort": [0]});
        let document = document_from_hit(&hit).unwrap();
        assert_eq!(
            document.key,
            json!({"_index": "orders", "_id": "1", "_seq_no": 0, "_primary_term": 1})
        );
        assert_eq!(document.body, json!({"total": 9.5}));
    }

    #[test]
    fn page_sizes_are_bounded() {
        assert_eq!(page_size(0), DEFAULT_PAGE_SIZE);
        assert_eq!(page_size(25), 25);
        assert_eq!(page_size(u32::MAX), MAX_PAGE_SIZE);
    }
}
