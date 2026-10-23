/*
 * @author t0bx
 * @since 23.10.2026
 */

use serde::Deserialize;
use serde_json::value::RawValue;

/// One page of the client protocol's `QueryResults` document. Only the fields the
/// driver uses are declared; `data` cells stay raw JSON so exact numbers survive.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueryResults {
    pub(crate) next_uri: Option<String>,
    pub(crate) columns: Option<Vec<ResultColumn>>,
    pub(crate) data: Option<Vec<Vec<Box<RawValue>>>>,
    pub(crate) error: Option<QueryError>,
    pub(crate) update_type: Option<String>,
    pub(crate) update_count: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ResultColumn {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) type_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueryError {
    pub(crate) message: String,
    pub(crate) error_name: Option<String>,
    pub(crate) error_type: Option<String>,
    pub(crate) error_location: Option<ErrorLocation>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ErrorLocation {
    pub(crate) line_number: usize,
    pub(crate) column_number: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerDescription {
    pub(crate) node_version: NodeVersion,
    #[serde(default)]
    pub(crate) starting: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NodeVersion {
    pub(crate) version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_results_keep_raw_cells() {
        let page: QueryResults = serde_json::from_str(
            r#"{"id":"q","nextUri":"http://h/v1/statement/executing/q/1",
                "columns":[{"name":"n","type":"decimal(38,2)","typeSignature":{}}],
                "data":[[12345678901234567890123456789012.34]],"stats":{"state":"RUNNING"}}"#,
        )
        .unwrap();
        let data = page.data.unwrap();
        assert_eq!(data[0][0].get(), "12345678901234567890123456789012.34");
        assert_eq!(page.columns.unwrap()[0].type_name, "decimal(38,2)");
        assert!(page.next_uri.is_some());
    }
}
