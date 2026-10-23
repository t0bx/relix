/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::TrinoConnection;
use relix_core::{DatabaseInfo, Dialect, ObjectInfo, ObjectKind, TableRef, Value};
use relix_driver_api::DriverError;

pub(crate) fn text_at(row: &[Value], index: usize) -> String {
    row.get(index)
        .and_then(Value::to_plain_string)
        .unwrap_or_default()
}

pub(crate) fn kind_of_table_type(table_type: &str) -> ObjectKind {
    match table_type {
        "VIEW" => ObjectKind::View,
        _ => ObjectKind::Table,
    }
}

pub(crate) fn information_schema(catalog: &str, relation: &str) -> String {
    format!(
        "{}.information_schema.{relation}",
        Dialect::Trino.quote_identifier(catalog)
    )
}

pub(crate) fn literal(text: &str) -> String {
    Dialect::Trino.quote_literal(text)
}

impl TrinoConnection {
    pub(crate) async fn list_catalogs(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        let default = self.initial_session.catalog.as_deref();
        let batch = self.query_rows("SHOW CATALOGS").await?;
        Ok(batch
            .rows
            .iter()
            .map(|row| {
                let name = text_at(row, 0);
                DatabaseInfo {
                    is_default: Some(name.as_str()) == default,
                    name,
                }
            })
            .collect())
    }

    pub(crate) async fn list_schemas(&self, catalog: &str) -> Result<Vec<String>, DriverError> {
        let sql = format!(
            "SELECT schema_name FROM {} ORDER BY schema_name",
            information_schema(catalog, "schemata")
        );
        let batch = self.query_rows(&sql).await?;
        Ok(batch.rows.iter().map(|row| text_at(row, 0)).collect())
    }

    pub(crate) async fn list_objects(
        &self,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        let sql = format!(
            "SELECT table_name, table_type FROM {} WHERE table_schema = {} ORDER BY table_name",
            information_schema(catalog, "tables"),
            literal(schema)
        );
        let batch = self.query_rows(&sql).await?;
        Ok(batch
            .rows
            .iter()
            .map(|row| ObjectInfo {
                object: TableRef::new(
                    Some(catalog.to_owned()),
                    Some(schema.to_owned()),
                    text_at(row, 0),
                ),
                kind: kind_of_table_type(&text_at(row, 1)),
                comment: None,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_and_literals_are_quoted() {
        assert_eq!(
            information_schema("my\"cat", "tables"),
            "\"my\"\"cat\".information_schema.tables"
        );
        assert_eq!(literal("o'neil"), "'o''neil'");
        assert_eq!(kind_of_table_type("VIEW"), ObjectKind::View);
        assert_eq!(kind_of_table_type("BASE TABLE"), ObjectKind::Table);
    }
}
