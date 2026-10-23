/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::{ClickHouseClient, QueryRequest};
use crate::metadata_rows::optional_text_at;
use relix_core::{Dialect, TableRef};
use relix_driver_api::{DriverError, ParamStyle, SqlBuilder};

const DICTIONARY_ENGINE: &str = "Dictionary";

/// Reads the DDL with `SHOW CREATE`; dictionaries need `SHOW CREATE DICTIONARY` to
/// return their source and layout instead of the table facade.
pub(crate) async fn show_create(
    client: &ClickHouseClient,
    table: &TableRef,
    engine: &str,
) -> Result<Option<String>, DriverError> {
    let object = if engine == DICTIONARY_ENGINE {
        "DICTIONARY"
    } else {
        "TABLE"
    };
    let qualified = SqlBuilder::new(Dialect::ClickHouse, ParamStyle::Inline).qualified_table(table);
    let sql = format!("SHOW CREATE {object} {qualified}");
    let batch = client.rows(&QueryRequest::new(&sql)).await?;
    Ok(batch.rows.first().and_then(|row| optional_text_at(row, 0)))
}
