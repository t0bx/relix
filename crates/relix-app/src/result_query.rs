/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::input::LineInput;
use relix_core::{SortDirection, SortKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultQuery {
    pub sort: Vec<SortKey>,
    pub filter_input: LineInput,
    pub applied_filter: Option<String>,
    pub offset: u64,
    pub page_size: u32,
    pub has_more: bool,
    pub total_rows: Option<u64>,
}

impl ResultQuery {
    pub fn new(page_size: u32) -> Self {
        Self {
            sort: Vec::new(),
            filter_input: LineInput::default(),
            applied_filter: None,
            offset: 0,
            page_size: page_size.max(1),
            has_more: false,
            total_rows: None,
        }
    }

    /// Single-column sort cycling: ascending → descending → unsorted.
    pub fn cycle_sort(&mut self, column: &str) {
        let current = self
            .sort
            .iter()
            .find(|key| key.column == column)
            .map(|key| key.direction);
        self.sort.clear();
        match current {
            None => self.sort.push(SortKey {
                column: column.to_owned(),
                direction: SortDirection::Ascending,
            }),
            Some(SortDirection::Ascending) => self.sort.push(SortKey {
                column: column.to_owned(),
                direction: SortDirection::Descending,
            }),
            Some(SortDirection::Descending) => {}
        }
        self.offset = 0;
    }

    /// Multi-column sort: appends the column ascending, flips it to descending on the
    /// second press and removes it on the third, keeping the other rules in place.
    pub fn add_sort(&mut self, column: &str) {
        match self.sort.iter().position(|key| key.column == column) {
            None => self.sort.push(SortKey {
                column: column.to_owned(),
                direction: SortDirection::Ascending,
            }),
            Some(index) if self.sort[index].direction == SortDirection::Ascending => {
                self.sort[index].direction = SortDirection::Descending;
            }
            Some(index) => {
                self.sort.remove(index);
            }
        }
        self.offset = 0;
    }

    pub fn remove_sort(&mut self, index: usize) {
        if index < self.sort.len() {
            self.sort.remove(index);
            self.offset = 0;
        }
    }

    pub fn toggle_sort_direction(&mut self, index: usize) {
        if let Some(key) = self.sort.get_mut(index) {
            key.direction = key.direction.toggled();
            self.offset = 0;
        }
    }

    pub fn move_sort(&mut self, index: usize, up: bool) -> usize {
        let target = if up {
            index.checked_sub(1)
        } else {
            (index + 1 < self.sort.len()).then_some(index + 1)
        };
        match target {
            Some(target) => {
                self.sort.swap(index, target);
                self.offset = 0;
                target
            }
            None => index,
        }
    }

    pub fn sort_position(&self, column: &str) -> Option<(usize, SortDirection)> {
        self.sort
            .iter()
            .position(|key| key.column == column)
            .map(|index| (index, self.sort[index].direction))
    }

    pub fn apply_filter_input(&mut self) -> Option<String> {
        let text = self.filter_input.text().trim().to_owned();
        self.applied_filter = (!text.is_empty()).then(|| text.clone());
        self.offset = 0;
        self.total_rows = None;
        self.applied_filter.clone()
    }

    pub fn next_page(&mut self) -> bool {
        if !self.has_more {
            return false;
        }
        self.offset += u64::from(self.page_size);
        true
    }

    pub fn previous_page(&mut self) -> bool {
        if self.offset == 0 {
            return false;
        }
        self.offset = self.offset.saturating_sub(u64::from(self.page_size));
        true
    }

    pub fn page_label(&self, rows_on_page: usize) -> String {
        let first = if rows_on_page == 0 {
            0
        } else {
            self.offset + 1
        };
        let last = self.offset + rows_on_page as u64;
        let total = match (self.total_rows, self.has_more) {
            (Some(total), _) => total.to_string(),
            (None, true) => "?".to_owned(),
            (None, false) => last.to_string(),
        };
        format!("{first}-{last} of {total}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_sort_cycles() {
        let mut query = ResultQuery::new(50);
        query.cycle_sort("a");
        assert_eq!(
            query.sort_position("a"),
            Some((0, SortDirection::Ascending))
        );
        query.cycle_sort("a");
        assert_eq!(
            query.sort_position("a"),
            Some((0, SortDirection::Descending))
        );
        query.cycle_sort("a");
        assert!(query.sort.is_empty());
    }

    #[test]
    fn multi_sort_rules_can_be_edited() {
        let mut query = ResultQuery::new(50);
        query.add_sort("a");
        query.add_sort("b");
        query.add_sort("b");
        assert_eq!(
            query.sort_position("b"),
            Some((1, SortDirection::Descending))
        );
        assert_eq!(query.move_sort(1, true), 0);
        assert_eq!(query.sort[0].column, "b");
        query.toggle_sort_direction(0);
        assert_eq!(query.sort[0].direction, SortDirection::Ascending);
        query.remove_sort(0);
        assert_eq!(query.sort.len(), 1);
        query.add_sort("a");
        query.add_sort("a");
        assert!(query.sort.is_empty());
    }

    #[test]
    fn paging_and_labels() {
        let mut query = ResultQuery::new(10);
        assert!(!query.next_page());
        query.has_more = true;
        assert!(query.next_page());
        assert_eq!(query.page_label(10), "11-20 of ?");
        query.has_more = false;
        assert_eq!(query.page_label(4), "11-14 of 14");
        assert!(query.previous_page());
        assert!(!query.previous_page());
        query.filter_input.set("  a > 1 ");
        assert_eq!(query.apply_filter_input().as_deref(), Some("a > 1"));
    }
}
