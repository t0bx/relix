/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::session_context::StatementContext;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub(crate) struct StatementRequest<'a> {
    pub statement: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(flatten)]
    pub context: &'a StatementContext,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ResultSet {
    pub code: Option<String>,
    pub message: Option<String>,
    pub statement_handle: Option<String>,
    pub result_set_meta_data: Option<ResultSetMetaData>,
    pub data: Vec<Vec<serde_json::Value>>,
    pub stats: Option<DmlStats>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ResultSetMetaData {
    pub num_rows: u64,
    pub partition_info: Vec<PartitionInfo>,
    pub row_type: Vec<RowType>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct PartitionInfo {
    pub row_count: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct RowType {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub scale: Option<u32>,
    pub precision: Option<u32>,
    pub nullable: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct DmlStats {
    pub num_rows_inserted: u64,
    pub num_rows_updated: u64,
    pub num_rows_deleted: u64,
}

impl DmlStats {
    pub(crate) fn affected_rows(&self) -> u64 {
        self.num_rows_inserted + self.num_rows_updated + self.num_rows_deleted
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct QueryStatus {
    pub code: Option<String>,
    pub sql_state: Option<String>,
    pub message: Option<String>,
    pub statement_handle: Option<String>,
}
