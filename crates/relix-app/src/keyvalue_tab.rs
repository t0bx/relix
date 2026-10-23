/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::input::LineInput;
use crate::requests::RequestId;
use relix_core::DataSourceId;
use relix_core::keyvalue::{KeyDescription, KeyName, KeyType, ValuePage};
use relix_query::RecentList;

pub const KEY_SCAN_BATCH: u32 = 200;
pub const VALUE_PAGE_SIZE: u32 = 100;
pub const STRING_PREVIEW_BYTES: u32 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyValueFocus {
    #[default]
    Keys,
    Pattern,
    Value,
    Console,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValueItem {
    pub index: Option<u64>,
    pub field: Option<Vec<u8>>,
    pub value: Vec<u8>,
    pub score: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValueContent {
    Text {
        bytes: Vec<u8>,
        total_length: u64,
    },
    Items {
        items: Vec<ValueItem>,
        next_cursor: Option<u64>,
    },
    Unsupported(String),
}

impl ValueContent {
    /// Appends a fetched page; collections accumulate so large values are browsed in
    /// chunks instead of being loaded at once.
    pub fn append(&mut self, page: ValuePage) {
        let incoming = Self::from_page(page);
        match (self, incoming) {
            (
                Self::Items { items, next_cursor },
                Self::Items {
                    items: more,
                    next_cursor: next,
                },
            ) => {
                items.extend(more);
                *next_cursor = next;
            }
            (this, other) => *this = other,
        }
    }

    pub fn from_page(page: ValuePage) -> Self {
        let item = |field: Option<Vec<u8>>,
                    value: Vec<u8>,
                    score: Option<f64>,
                    index: Option<u64>| ValueItem {
            index,
            field,
            value,
            score,
        };
        match page {
            ValuePage::String {
                bytes,
                total_length,
            } => Self::Text {
                bytes,
                total_length,
            },
            ValuePage::List {
                items,
                start,
                next_cursor,
            } => Self::Items {
                items: items
                    .into_iter()
                    .enumerate()
                    .map(|(offset, value)| item(None, value, None, Some(start + offset as u64)))
                    .collect(),
                next_cursor,
            },
            ValuePage::Set {
                members,
                next_cursor,
            } => Self::Items {
                items: members
                    .into_iter()
                    .map(|value| item(None, value, None, None))
                    .collect(),
                next_cursor,
            },
            ValuePage::SortedSet {
                entries,
                start,
                next_cursor,
            } => Self::Items {
                items: entries
                    .into_iter()
                    .enumerate()
                    .map(|(offset, (member, score))| {
                        item(None, member, Some(score), Some(start + offset as u64))
                    })
                    .collect(),
                next_cursor,
            },
            ValuePage::Hash {
                entries,
                next_cursor,
            } => Self::Items {
                items: entries
                    .into_iter()
                    .map(|(field, value)| item(Some(field), value, None, None))
                    .collect(),
                next_cursor,
            },
            ValuePage::Stream {
                entries,
                next_cursor,
            } => Self::Items {
                items: entries
                    .into_iter()
                    .map(|entry| {
                        let fields: Vec<String> = entry
                            .fields
                            .iter()
                            .map(|(field, value)| {
                                format!(
                                    "{}={}",
                                    String::from_utf8_lossy(field),
                                    String::from_utf8_lossy(value)
                                )
                            })
                            .collect();
                        item(
                            Some(entry.id.into_bytes()),
                            fields.join(" ").into_bytes(),
                            None,
                            None,
                        )
                    })
                    .collect(),
                next_cursor,
            },
            ValuePage::Unsupported { key_type } => Self::Unsupported(key_type),
        }
    }

    pub fn next_cursor(&self) -> Option<u64> {
        match self {
            Self::Items { next_cursor, .. } => *next_cursor,
            _ => None,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Items { items, .. } => items.len(),
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLine {
    pub text: String,
    pub is_error: bool,
    pub is_input: bool,
}

#[derive(Debug, Clone)]
pub struct KeyValueTab {
    pub data_source: DataSourceId,
    pub pattern: LineInput,
    pub keys: Vec<KeyName>,
    pub scan_cursor: u64,
    pub scan_complete: bool,
    pub scanning: Option<RequestId>,
    pub selected_key: usize,
    pub key_scroll: usize,
    pub focus: KeyValueFocus,
    pub description: Option<KeyDescription>,
    pub value: Option<ValueContent>,
    pub value_loading: Option<RequestId>,
    pub selected_item: usize,
    pub item_scroll: usize,
    pub console_input: LineInput,
    pub console_output: Vec<ConsoleLine>,
    pub console_history: RecentList,
    pub history_index: Option<usize>,
    pub server_details: Vec<(String, String)>,
}

impl KeyValueTab {
    pub fn new(data_source: DataSourceId) -> Self {
        Self {
            data_source,
            pattern: LineInput::with_text("*"),
            keys: Vec::new(),
            scan_cursor: 0,
            scan_complete: false,
            scanning: None,
            selected_key: 0,
            key_scroll: 0,
            focus: KeyValueFocus::Keys,
            description: None,
            value: None,
            value_loading: None,
            selected_item: 0,
            item_scroll: 0,
            console_input: LineInput::default(),
            console_output: Vec::new(),
            console_history: RecentList::new(100),
            history_index: None,
            server_details: Vec::new(),
        }
    }

    pub fn selected_key_name(&self) -> Option<&KeyName> {
        self.keys.get(self.selected_key)
    }

    pub fn reset_scan(&mut self) {
        self.keys.clear();
        self.scan_cursor = 0;
        self.scan_complete = false;
        self.selected_key = 0;
        self.key_scroll = 0;
    }

    pub fn key_type(&self) -> Option<&KeyType> {
        self.description
            .as_ref()
            .map(|description| &description.key_type)
    }

    pub fn push_console(&mut self, text: String, is_error: bool, is_input: bool) {
        self.console_output.push(ConsoleLine {
            text,
            is_error,
            is_input,
        });
        let overflow = self.console_output.len().saturating_sub(2000);
        self.console_output.drain(..overflow);
    }

    pub fn selected_item(&self) -> Option<&ValueItem> {
        match &self.value {
            Some(ValueContent::Items { items, .. }) => items.get(self.selected_item),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_accumulate_for_collections() {
        let mut content = ValueContent::from_page(ValuePage::List {
            items: vec![b"a".to_vec(), b"b".to_vec()],
            start: 0,
            next_cursor: Some(2),
        });
        content.append(ValuePage::List {
            items: vec![b"c".to_vec()],
            start: 2,
            next_cursor: None,
        });
        assert_eq!(content.len(), 3);
        assert_eq!(content.next_cursor(), None);
        let ValueContent::Items { items, .. } = &content else {
            panic!("expected items");
        };
        assert_eq!(items[2].index, Some(2));
    }

    #[test]
    fn hash_and_zset_items_keep_fields_and_scores() {
        let hash = ValueContent::from_page(ValuePage::Hash {
            entries: vec![(b"f".to_vec(), b"v".to_vec())],
            next_cursor: Some(9),
        });
        assert_eq!(hash.next_cursor(), Some(9));
        let zset = ValueContent::from_page(ValuePage::SortedSet {
            entries: vec![(b"m".to_vec(), 1.5)],
            start: 0,
            next_cursor: None,
        });
        let ValueContent::Items { items, .. } = zset else {
            panic!("expected items");
        };
        assert_eq!(items[0].score, Some(1.5));
    }
}
