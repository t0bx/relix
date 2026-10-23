/*
 * @author t0bx
 * @since 23.10.2026
 */

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn toggled(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }

    pub fn sql_keyword(self) -> &'static str {
        match self {
            Self::Ascending => "ASC",
            Self::Descending => "DESC",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortKey {
    pub column: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableQuery {
    pub filter: Option<String>,
    pub sort: Vec<SortKey>,
    #[serde(default)]
    pub tiebreaker: Vec<String>,
    pub offset: u64,
    pub limit: u32,
}

impl Default for TableQuery {
    fn default() -> Self {
        Self {
            filter: None,
            sort: Vec::new(),
            tiebreaker: Vec::new(),
            offset: 0,
            limit: 200,
        }
    }
}
