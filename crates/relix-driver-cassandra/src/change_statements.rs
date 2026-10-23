/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::literals::{is_counter, literal};
use crate::schema_columns::{ColumnKind, SchemaColumn};
use crate::table_names::{qualified_name, quote};
use relix_core::{CellChange, RowChange, Value};
use relix_driver_api::{DriverError, ErrorKind};

pub(crate) struct ChangeTarget<'a> {
    pub(crate) keyspace: &'a str,
    pub(crate) table: &'a str,
    pub(crate) columns: &'a [SchemaColumn],
}

impl ChangeTarget<'_> {
    /// Builds a lightweight-transaction statement for one row change. Updates are keyed
    /// by the full primary key and guarded with `IF column = original` for every changed
    /// cell, inserts use `IF NOT EXISTS` and deletes `IF EXISTS`, so a statement that is
    /// not applied means the row changed concurrently. Updates that only touch static
    /// columns are keyed by the partition key, as CQL requires.
    pub(crate) fn statement(&self, change: &RowChange) -> Result<String, DriverError> {
        if let Some(counter) = self
            .columns
            .iter()
            .find(|column| is_counter(&column.data_type))
        {
            return Err(DriverError::unsupported(format!(
                "tables with counter columns ({}) cannot be edited in the grid; \
                 use UPDATE ... SET c = c + n in the console",
                counter.name
            )));
        }
        match change {
            RowChange::Update { key, cells } => self.update(key, cells),
            RowChange::Insert { values } => self.insert(values),
            RowChange::Delete { key } => self.delete(key),
        }
    }

    fn update(&self, key: &[(String, Value)], cells: &[CellChange]) -> Result<String, DriverError> {
        if cells.is_empty() {
            return Err(DriverError::internal("update without changed cells"));
        }
        let mut assignments = Vec::with_capacity(cells.len());
        let mut conditions = Vec::with_capacity(cells.len());
        for cell in cells {
            let column = self.column(&cell.column)?;
            if column.kind.is_primary_key() {
                return Err(DriverError::unsupported(format!(
                    "primary key column {} cannot be changed in CQL; \
                     delete the row and insert it with the new key instead",
                    column.name
                )));
            }
            let name = quote(&column.name);
            assignments.push(format!("{name} = {}", self.literal(column, &cell.updated)?));
            conditions.push(format!(
                "{name} = {}",
                self.literal(column, &cell.original)?
            ));
        }
        let only_static = cells.iter().all(|cell| {
            self.column(&cell.column)
                .is_ok_and(|column| column.kind == ColumnKind::Static)
        });
        let condition = self.key_condition(key, only_static)?;
        Ok(format!(
            "UPDATE {} SET {} WHERE {condition} IF {}",
            qualified_name(self.keyspace, self.table),
            assignments.join(", "),
            conditions.join(" AND ")
        ))
    }

    fn insert(&self, values: &[(String, Value)]) -> Result<String, DriverError> {
        let mut names = Vec::new();
        let mut literals = Vec::new();
        for (name, value) in values {
            let column = self.column(name)?;
            if value.is_null() && !column.kind.is_primary_key() {
                continue;
            }
            names.push(quote(&column.name));
            literals.push(self.literal(column, value)?);
        }
        if names.is_empty() {
            return Err(DriverError::query(
                "a new row needs values for its primary key",
            ));
        }
        Ok(format!(
            "INSERT INTO {} ({}) VALUES ({}) IF NOT EXISTS",
            qualified_name(self.keyspace, self.table),
            names.join(", "),
            literals.join(", ")
        ))
    }

    fn delete(&self, key: &[(String, Value)]) -> Result<String, DriverError> {
        Ok(format!(
            "DELETE FROM {} WHERE {} IF EXISTS",
            qualified_name(self.keyspace, self.table),
            self.key_condition(key, false)?
        ))
    }

    fn key_condition(
        &self,
        key: &[(String, Value)],
        partition_only: bool,
    ) -> Result<String, DriverError> {
        let required: Vec<&SchemaColumn> = self
            .columns
            .iter()
            .filter(|column| match column.kind {
                ColumnKind::PartitionKey { .. } => true,
                ColumnKind::Clustering { .. } => !partition_only,
                _ => false,
            })
            .collect();
        let mut terms = Vec::with_capacity(required.len());
        for column in required {
            let value = key
                .iter()
                .find(|(name, _)| name == &column.name)
                .map(|(_, value)| value)
                .filter(|value| !value.is_null())
                .ok_or_else(|| {
                    DriverError::new(
                        ErrorKind::AmbiguousRow,
                        format!(
                            "rows are addressed by their full primary key, but {} is missing",
                            column.name
                        ),
                    )
                })?;
            terms.push(format!(
                "{} = {}",
                quote(&column.name),
                self.literal(column, value)?
            ));
        }
        Ok(terms.join(" AND "))
    }

    fn column(&self, name: &str) -> Result<&SchemaColumn, DriverError> {
        self.columns
            .iter()
            .find(|column| column.name == name)
            .ok_or_else(|| {
                DriverError::query(format!("column {name} does not exist in {}", self.table))
            })
    }

    fn literal(&self, column: &SchemaColumn, value: &Value) -> Result<String, DriverError> {
        literal(&column.name, &column.data_type, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn columns() -> Vec<SchemaColumn> {
        let column = |name: &str, kind: ColumnKind, data_type: &str| SchemaColumn {
            name: name.into(),
            kind,
            data_type: data_type.into(),
        };
        vec![
            column("tenant", ColumnKind::PartitionKey { position: 0 }, "int"),
            column(
                "id",
                ColumnKind::Clustering {
                    position: 0,
                    descending: false,
                },
                "uuid",
            ),
            column("plan", ColumnKind::Static, "text"),
            column("name", ColumnKind::Regular, "text"),
        ]
    }

    const ID: &str = "123e4567-e89b-12d3-a456-426614174000";

    fn key() -> Vec<(String, Value)> {
        vec![
            ("tenant".into(), Value::Int(1)),
            ("id".into(), Value::Text(ID.into())),
        ]
    }

    #[test]
    fn updates_are_guarded_by_original_values() {
        let columns = columns();
        let target = ChangeTarget {
            keyspace: "app",
            table: "users",
            columns: &columns,
        };
        let change = RowChange::Update {
            key: key(),
            cells: vec![CellChange {
                column: "name".into(),
                original: Value::Null,
                updated: Value::Text("Ann".into()),
            }],
        };
        assert_eq!(
            target.statement(&change).unwrap(),
            format!(
                "UPDATE \"app\".\"users\" SET \"name\" = 'Ann' \
                 WHERE \"tenant\" = 1 AND \"id\" = {ID} IF \"name\" = null"
            )
        );
    }

    #[test]
    fn static_only_updates_use_the_partition_key() {
        let columns = columns();
        let target = ChangeTarget {
            keyspace: "app",
            table: "users",
            columns: &columns,
        };
        let change = RowChange::Update {
            key: key(),
            cells: vec![CellChange {
                column: "plan".into(),
                original: Value::Text("free".into()),
                updated: Value::Text("pro".into()),
            }],
        };
        assert_eq!(
            target.statement(&change).unwrap(),
            "UPDATE \"app\".\"users\" SET \"plan\" = 'pro' WHERE \"tenant\" = 1 IF \"plan\" = 'free'"
        );
    }

    #[test]
    fn inserts_and_deletes_are_conditional() {
        let columns = columns();
        let target = ChangeTarget {
            keyspace: "app",
            table: "users",
            columns: &columns,
        };
        let mut values = key();
        values.push(("name".into(), Value::Null));
        assert_eq!(
            target.statement(&RowChange::Insert { values }).unwrap(),
            format!(
                "INSERT INTO \"app\".\"users\" (\"tenant\", \"id\") VALUES (1, {ID}) IF NOT EXISTS"
            )
        );
        assert_eq!(
            target.statement(&RowChange::Delete { key: key() }).unwrap(),
            format!(
                "DELETE FROM \"app\".\"users\" WHERE \"tenant\" = 1 AND \"id\" = {ID} IF EXISTS"
            )
        );
    }

    #[test]
    fn incomplete_keys_and_key_updates_are_refused() {
        let columns = columns();
        let target = ChangeTarget {
            keyspace: "app",
            table: "users",
            columns: &columns,
        };
        let partial = RowChange::Delete {
            key: vec![("tenant".into(), Value::Int(1))],
        };
        assert_eq!(
            target.statement(&partial).unwrap_err().kind,
            ErrorKind::AmbiguousRow
        );
        let key_change = RowChange::Update {
            key: key(),
            cells: vec![CellChange {
                column: "tenant".into(),
                original: Value::Int(1),
                updated: Value::Int(2),
            }],
        };
        assert_eq!(
            target.statement(&key_change).unwrap_err().kind,
            ErrorKind::Unsupported
        );
    }
}
