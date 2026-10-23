/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::input::LineInput;
use crate::requests::RequestId;
use relix_core::{
    CollectionDetails, CollectionKind, CollectionRef, DataSourceId, Document, DocumentQuery,
};

pub const DOCUMENT_PAGE_SIZE: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocumentFocus {
    #[default]
    List,
    Filter,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentTab {
    pub data_source: DataSourceId,
    pub collection: CollectionRef,
    pub kind: CollectionKind,
    pub filter: LineInput,
    pub filter_hint: String,
    pub documents: Vec<Document>,
    pub selected: usize,
    pub detail_scroll: usize,
    pub page: usize,
    pub next_page: Option<String>,
    pub total: Option<u64>,
    pub loading: Option<RequestId>,
    pub error: Option<String>,
    pub details: Option<CollectionDetails>,
    pub focus: DocumentFocus,
    page_tokens: Vec<Option<String>>,
}

impl DocumentTab {
    pub fn new(data_source: DataSourceId, collection: CollectionRef, kind: CollectionKind) -> Self {
        Self {
            data_source,
            collection,
            kind,
            filter: LineInput::default(),
            filter_hint: String::new(),
            documents: Vec::new(),
            selected: 0,
            detail_scroll: 0,
            page: 0,
            next_page: None,
            total: None,
            loading: None,
            error: None,
            details: None,
            focus: DocumentFocus::List,
            page_tokens: vec![None],
        }
    }

    pub fn title(&self) -> String {
        self.collection.name.clone()
    }

    pub fn selected_document(&self) -> Option<&Document> {
        self.documents.get(self.selected)
    }

    pub fn selected_json(&self) -> Option<String> {
        self.selected_document()
            .and_then(|document| serde_json::to_string_pretty(&document.body).ok())
    }

    /// Builds the query for the current page. The filter field holds a JSON object in the
    /// engine's native filter language; an empty field means "everything".
    pub fn query(&self) -> Result<DocumentQuery, String> {
        let text = self.filter.text().trim();
        let filter = if text.is_empty() {
            None
        } else {
            let value: serde_json::Value = serde_json::from_str(text)
                .map_err(|error| format!("filter is not valid JSON: {error}"))?;
            if !value.is_object() {
                return Err("the filter must be a JSON object".into());
            }
            Some(value)
        };
        Ok(DocumentQuery {
            filter,
            sort: None,
            projection: None,
            page_token: self.page_tokens.get(self.page).cloned().flatten(),
            limit: DOCUMENT_PAGE_SIZE,
        })
    }

    pub fn reset_paging(&mut self) {
        self.page = 0;
        self.page_tokens = vec![None];
        self.next_page = None;
        self.selected = 0;
        self.detail_scroll = 0;
    }

    pub fn go_next(&mut self) -> bool {
        let Some(token) = self.next_page.clone() else {
            return false;
        };
        self.page += 1;
        self.page_tokens.truncate(self.page);
        self.page_tokens.push(Some(token));
        self.selected = 0;
        self.detail_scroll = 0;
        true
    }

    pub fn go_previous(&mut self) -> bool {
        if self.page == 0 {
            return false;
        }
        self.page -= 1;
        self.selected = 0;
        self.detail_scroll = 0;
        true
    }

    pub fn move_selection(&mut self, delta: isize) {
        let last = self.documents.len().saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(delta).min(last);
        self.detail_scroll = 0;
    }

    pub fn page_label(&self) -> String {
        let total = self
            .total
            .map(|total| format!(" of {total}"))
            .unwrap_or_default();
        let more = if self.next_page.is_some() {
            " · ] next"
        } else {
            ""
        };
        let back = if self.page > 0 { " · [ previous" } else { "" };
        format!(
            "page {} · {} documents{total}{more}{back}",
            self.page + 1,
            self.documents.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tab() -> DocumentTab {
        DocumentTab::new(
            DataSourceId::new(),
            CollectionRef::new("shop", "orders"),
            CollectionKind::Collection,
        )
    }

    #[test]
    fn filters_must_be_json_objects() {
        let mut tab = tab();
        assert_eq!(tab.query().unwrap().filter, None);
        tab.filter.set(r#"{"status": "open"}"#);
        assert_eq!(tab.query().unwrap().filter, Some(json!({"status": "open"})));
        tab.filter.set("[1]");
        assert!(tab.query().is_err());
        tab.filter.set("{broken");
        assert!(tab.query().unwrap_err().contains("not valid JSON"));
    }

    #[test]
    fn page_tokens_are_remembered_for_going_back() {
        let mut tab = tab();
        assert!(!tab.go_next());
        tab.next_page = Some("50".into());
        assert!(tab.go_next());
        assert_eq!(tab.query().unwrap().page_token.as_deref(), Some("50"));
        tab.next_page = Some("100".into());
        assert!(tab.go_next());
        assert!(tab.go_previous());
        assert_eq!(tab.query().unwrap().page_token.as_deref(), Some("50"));
        assert!(tab.go_previous());
        assert_eq!(tab.query().unwrap().page_token, None);
        assert!(!tab.go_previous());
    }
}
