/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::BigQueryConnection;
use crate::values::cell_value;
use relix_core::{Dialect, ObjectInfo, ObjectKind, Value};
use relix_driver_api::DriverError;

/// Reads the DDL BigQuery keeps in `INFORMATION_SCHEMA.TABLES` or `.ROUTINES` of the
/// object's dataset.
pub(crate) async fn ddl(
    connection: &BigQueryConnection,
    object: &ObjectInfo,
) -> Result<Option<String>, DriverError> {
    let (view, name_column) = match object.kind {
        ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView => {
            ("TABLES", "table_name")
        }
        ObjectKind::Function | ObjectKind::Procedure => ("ROUTINES", "routine_name"),
        _ => return Ok(None),
    };
    let (project, dataset) = connection.table_scope(&object.object)?;
    let dialect = Dialect::BigQuery;
    let sql = format!(
        "SELECT ddl FROM {}.{}.INFORMATION_SCHEMA.{view} WHERE {name_column} = {}",
        dialect.quote_identifier(&project),
        dialect.quote_identifier(&dataset),
        dialect.quote_literal(&object.object.name)
    );
    let results = connection.query(&sql, 1).await?;
    let fields = results.schema.unwrap_or_default().fields;
    let ddl = results
        .rows
        .first()
        .zip(fields.first())
        .and_then(|(row, field)| row.f.first().map(|cell| cell_value(&cell.v, field)));
    Ok(match ddl {
        Some(Value::Text(text)) => Some(text),
        _ => None,
    })
}
