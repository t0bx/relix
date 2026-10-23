/*
 * @author t0bx
 * @since 23.10.2026
 */

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueryRequest {
    pub query: String,
    pub use_legacy_sql: bool,
    pub max_results: u32,
    pub timeout_ms: u64,
    pub request_id: String,
    pub format_options: FormatOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_dataset: Option<DatasetReference>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub create_session: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub connection_properties: Vec<ConnectionProperty>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_bytes_billed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_timeout_ms: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FormatOptions {
    pub use_int64_timestamp: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ConnectionProperty {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DatasetReference {
    pub project_id: String,
    pub dataset_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JobReference {
    pub project_id: String,
    pub job_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct QueryResults {
    pub schema: Option<TableSchema>,
    pub job_reference: Option<JobReference>,
    pub total_rows: Option<String>,
    pub page_token: Option<String>,
    pub rows: Vec<TableRow>,
    pub job_complete: Option<bool>,
    pub num_dml_affected_rows: Option<String>,
    pub session_info: Option<SessionInfo>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SessionInfo {
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TableDataPage {
    pub total_rows: Option<String>,
    pub page_token: Option<String>,
    pub rows: Vec<TableRow>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct TableSchema {
    pub fields: Vec<TableField>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TableField {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub mode: Option<String>,
    pub fields: Vec<TableField>,
    pub description: Option<String>,
    pub default_value_expression: Option<String>,
}

impl TableField {
    pub(crate) fn is_repeated(&self) -> bool {
        self.mode
            .as_deref()
            .is_some_and(|mode| mode.eq_ignore_ascii_case("REPEATED"))
    }

    pub(crate) fn is_required(&self) -> bool {
        self.mode
            .as_deref()
            .is_some_and(|mode| mode.eq_ignore_ascii_case("REQUIRED"))
    }

    pub(crate) fn is_record(&self) -> bool {
        matches!(
            self.field_type.to_ascii_uppercase().as_str(),
            "RECORD" | "STRUCT"
        )
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct TableRow {
    pub f: Vec<TableCell>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct TableCell {
    pub v: serde_json::Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct DatasetList {
    pub datasets: Vec<DatasetEntry>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DatasetEntry {
    pub dataset_reference: DatasetReference,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TableList {
    pub tables: Vec<TableEntry>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TableEntry {
    pub table_reference: TableReference,
    #[serde(rename = "type", default)]
    pub table_type: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TableReference {
    pub project_id: String,
    pub dataset_id: String,
    pub table_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct RoutineList {
    pub routines: Vec<RoutineEntry>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoutineEntry {
    pub routine_reference: RoutineReference,
    #[serde(default)]
    pub routine_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoutineReference {
    pub routine_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Table {
    #[serde(rename = "type")]
    pub table_type: Option<String>,
    pub schema: Option<TableSchema>,
    pub description: Option<String>,
    pub view: Option<ViewDefinition>,
    pub materialized_view: Option<ViewDefinition>,
    pub table_constraints: Option<TableConstraints>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ViewDefinition {
    pub query: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TableConstraints {
    pub primary_key: Option<PrimaryKey>,
    pub foreign_keys: Vec<ForeignKey>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct PrimaryKey {
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ForeignKey {
    pub name: Option<String>,
    pub referenced_table: TableReference,
    pub column_references: Vec<ColumnReference>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ColumnReference {
    pub referencing_column: String,
    pub referenced_column: String,
}
