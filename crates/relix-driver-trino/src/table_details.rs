/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::{information_schema, kind_of_table_type, literal, text_at};
use crate::connection::TrinoConnection;
use relix_core::Dialect;
use relix_core::{ColumnInfo, ObjectKind, TableDetails, TableRef, Value};
use relix_driver_api::{DriverError, ParamStyle, SqlBuilder};

/// Catalog, schema and name of a table, falling back to the session defaults.
pub(crate) struct QualifiedName {
    pub(crate) catalog: String,
    pub(crate) schema: String,
    pub(crate) name: String,
}

impl TrinoConnection {
    pub(crate) fn qualify(&self, table: &TableRef) -> Result<QualifiedName, DriverError> {
        let catalog = table
            .database
            .clone()
            .or_else(|| self.initial_session.catalog.clone())
            .ok_or_else(|| DriverError::query("no catalog selected for the table"))?;
        let schema = table
            .schema
            .clone()
            .or_else(|| self.initial_session.schema.clone())
            .ok_or_else(|| DriverError::query("no schema selected for the table"))?;
        Ok(QualifiedName {
            catalog,
            schema,
            name: table.name.clone(),
        })
    }

    /// Columns come from the catalog's `information_schema`; Trino exposes no primary,
    /// unique or foreign keys, so those lists stay empty.
    pub(crate) async fn load_table_details(
        &self,
        table: &TableRef,
    ) -> Result<TableDetails, DriverError> {
        let qualified = self.qualify(table)?;
        let kind = self.relation_kind(&qualified).await?.ok_or_else(|| {
            DriverError::query(format!(
                "table {}.{}.{} does not exist",
                qualified.catalog, qualified.schema, qualified.name
            ))
        })?;
        let sql = format!(
            "SELECT column_name, data_type, is_nullable, column_default FROM {} \
             WHERE table_schema = {} AND table_name = {} ORDER BY ordinal_position",
            information_schema(&qualified.catalog, "columns"),
            literal(&qualified.schema),
            literal(&qualified.name)
        );
        let columns = self
            .query_rows(&sql)
            .await?
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| column(index, row))
            .collect();
        let definition = self.definition(&qualified, kind).await?;
        Ok(TableDetails {
            table: TableRef::new(
                Some(qualified.catalog),
                Some(qualified.schema),
                qualified.name,
            ),
            kind,
            columns,
            primary_key: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            definition,
        })
    }

    pub(crate) async fn relation_kind(
        &self,
        qualified: &QualifiedName,
    ) -> Result<Option<ObjectKind>, DriverError> {
        let sql = format!(
            "SELECT table_type FROM {} WHERE table_schema = {} AND table_name = {}",
            information_schema(&qualified.catalog, "tables"),
            literal(&qualified.schema),
            literal(&qualified.name)
        );
        let batch = self.query_rows(&sql).await?;
        Ok(batch
            .rows
            .first()
            .map(|row| kind_of_table_type(&text_at(row, 0))))
    }

    pub(crate) async fn definition(
        &self,
        qualified: &QualifiedName,
        kind: ObjectKind,
    ) -> Result<Option<String>, DriverError> {
        let object = match kind {
            ObjectKind::View => "VIEW",
            _ => "TABLE",
        };
        let table = TableRef::new(
            Some(qualified.catalog.clone()),
            Some(qualified.schema.clone()),
            qualified.name.clone(),
        );
        let name = SqlBuilder::new(Dialect::Trino, ParamStyle::Inline).qualified_table(&table);
        let batch = self
            .query_rows(&format!("SHOW CREATE {object} {name}"))
            .await?;
        Ok(batch
            .rows
            .first()
            .map(|row| text_at(row, 0))
            .filter(|text| !text.is_empty()))
    }
}

fn column(index: usize, row: &[Value]) -> ColumnInfo {
    ColumnInfo {
        name: text_at(row, 0),
        data_type: text_at(row, 1),
        nullable: text_at(row, 2) != "NO",
        default_value: Some(text_at(row, 3)).filter(|value| !value.is_empty()),
        ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
        is_primary_key: false,
        is_auto_increment: false,
        comment: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_read_nullability_and_defaults() {
        let row = vec![
            Value::Text("nationkey".into()),
            Value::Text("bigint".into()),
            Value::Text("NO".into()),
            Value::Null,
        ];
        let info = column(0, &row);
        assert!(!info.nullable);
        assert_eq!(info.default_value, None);
        assert_eq!(info.ordinal, 1);
    }
}
