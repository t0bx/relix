/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::metadata::TableRef;
use crate::value::Value;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellChange {
    pub column: String,
    pub original: Value,
    pub updated: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RowChange {
    Update {
        key: Vec<(String, Value)>,
        cells: Vec<CellChange>,
    },
    Insert {
        values: Vec<(String, Value)>,
    },
    Delete {
        key: Vec<(String, Value)>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeSet {
    pub table: TableRef,
    pub changes: Vec<RowChange>,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChangeReport {
    pub updated: u64,
    pub inserted: u64,
    pub deleted: u64,
}
