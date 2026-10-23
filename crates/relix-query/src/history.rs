/*
 * @author t0bx
 * @since 23.10.2026
 */

use chrono::{DateTime, Utc};
use relix_core::DataSourceId;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub sql: String,
    pub data_source: Option<DataSourceId>,
    pub executed_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub success: bool,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryHistory {
    entries: VecDeque<HistoryEntry>,
    capacity: usize,
}

impl QueryHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    pub fn from_entries(entries: Vec<HistoryEntry>, capacity: usize) -> Self {
        let mut history = Self::new(capacity);
        for entry in entries {
            history.record(entry);
        }
        history
    }

    /// Records an execution. Re-running the statement that is already the newest entry
    /// for the same data source replaces it instead of filling the history with repeats.
    pub fn record(&mut self, entry: HistoryEntry) {
        let repeats_newest = self.entries.back().is_some_and(|newest| {
            newest.sql == entry.sql && newest.data_source == entry.data_source
        });
        if repeats_newest {
            self.entries.pop_back();
        }
        self.entries.push_back(entry);
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn newest_first(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.entries.iter().rev()
    }

    pub fn search<'a>(&'a self, needle: &'a str) -> impl Iterator<Item = &'a HistoryEntry> + 'a {
        let needle = needle.to_lowercase();
        self.newest_first()
            .filter(move |entry| needle.is_empty() || entry.sql.to_lowercase().contains(&needle))
    }

    /// Distinct successfully executed statements, newest first, for completion.
    pub fn fragments(&self, limit: usize) -> Vec<String> {
        let mut seen = Vec::new();
        for entry in self.newest_first().filter(|entry| entry.success) {
            let fragment = entry.sql.trim().to_owned();
            if !seen.contains(&fragment) {
                seen.push(fragment);
            }
            if seen.len() == limit {
                break;
            }
        }
        seen
    }

    pub fn entries(&self) -> Vec<HistoryEntry> {
        self.entries.iter().cloned().collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RecentList {
    items: VecDeque<String>,
    #[serde(default = "default_recent_capacity")]
    capacity: usize,
}

fn default_recent_capacity() -> usize {
    30
}

impl RecentList {
    pub fn new(capacity: usize) -> Self {
        Self {
            items: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    pub fn push(&mut self, item: &str) {
        let item = item.trim();
        if item.is_empty() {
            return;
        }
        self.items.retain(|existing| existing != item);
        self.items.push_front(item.to_owned());
        self.items.truncate(self.capacity.max(1));
    }

    pub fn items(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(String::as_str)
    }

    pub fn get(&self, index: usize) -> Option<&str> {
        self.items.get(index).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sql: &str, success: bool) -> HistoryEntry {
        HistoryEntry {
            sql: sql.into(),
            data_source: None,
            executed_at: Utc::now(),
            duration_ms: 1,
            success,
            summary: String::new(),
        }
    }

    #[test]
    fn collapses_repeats_and_respects_capacity() {
        let mut history = QueryHistory::new(3);
        history.record(entry("select 1", true));
        history.record(entry("select 1", true));
        history.record(entry("select 2", true));
        history.record(entry("select 3", false));
        history.record(entry("select 4", true));
        assert_eq!(history.len(), 3);
        let newest: Vec<_> = history.newest_first().map(|e| e.sql.as_str()).collect();
        assert_eq!(newest, vec!["select 4", "select 3", "select 2"]);
    }

    #[test]
    fn search_and_fragments() {
        let history = QueryHistory::from_entries(
            vec![
                entry("SELECT * FROM users", true),
                entry("select * from orders", true),
                entry("broken", false),
                entry("SELECT * FROM users", true),
            ],
            10,
        );
        assert_eq!(history.search("USERS").count(), 2);
        assert_eq!(
            history.fragments(5),
            vec!["SELECT * FROM users", "select * from orders"]
        );
    }

    #[test]
    fn recent_list_moves_items_to_front() {
        let mut recent = RecentList::new(2);
        recent.push("a = 1");
        recent.push("b = 2");
        recent.push("a = 1");
        recent.push("  ");
        recent.push("c = 3");
        assert_eq!(recent.items().collect::<Vec<_>>(), vec!["c = 3", "a = 1"]);
    }
}
