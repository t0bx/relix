/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::definitions::{DEFAULT_SCHEMA, database_of, relation_definition};
use crate::query::{query_all, text_list};
use duckdb::{Connection, params};
use relix_core::{ColumnInfo, ForeignKeyInfo, IndexInfo, TableDetails, TableRef};
use relix_driver_api::DriverError;

const COLUMNS_SQL: &str = "SELECT column_name, data_type, is_nullable, column_default, comment \
     FROM duckdb_columns() WHERE database_name = ? AND schema_name = ? AND table_name = ? \
     ORDER BY column_index";

const CONSTRAINTS_SQL: &str = "SELECT constraint_type, constraint_name, constraint_column_names, \
     referenced_table, referenced_column_names \
     FROM duckdb_constraints() WHERE database_name = ? AND schema_name = ? AND table_name = ? \
       AND constraint_type IN ('PRIMARY KEY', 'UNIQUE', 'FOREIGN KEY') \
     ORDER BY constraint_index";

const INDEXES_SQL: &str = "SELECT index_name, is_unique, is_primary, expressions \
     FROM duckdb_indexes() WHERE database_name = ? AND schema_name = ? AND table_name = ? \
     ORDER BY index_name";

struct Constraint {
    kind: String,
    name: String,
    columns: Vec<String>,
    referenced_table: Option<String>,
    referenced_columns: Vec<String>,
}

pub(crate) fn load_table_details(
    connection: &Connection,
    table: &TableRef,
) -> Result<TableDetails, DriverError> {
    let database = database_of(connection, table)?;
    let schema = table.schema.as_deref().unwrap_or(DEFAULT_SCHEMA);
    let (kind, definition) = relation_definition(connection, &database, schema, &table.name)?
        .ok_or_else(|| {
            DriverError::query(format!("table or view {} does not exist", table.name))
        })?;
    let constraints = load_constraints(connection, &database, schema, &table.name)?;
    let primary_key = constraints
        .iter()
        .find(|constraint| constraint.kind == "PRIMARY KEY")
        .map(|constraint| constraint.columns.clone())
        .unwrap_or_default();
    let columns = load_columns(connection, &database, schema, &table.name, &primary_key)?;
    let mut indexes = constraint_indexes(&constraints);
    indexes.extend(load_indexes(connection, &database, schema, &table.name)?);
    let foreign_keys = constraints
        .iter()
        .filter_map(|constraint| foreign_key(constraint, &database, schema))
        .collect();
    Ok(TableDetails {
        table: TableRef::new(Some(database), Some(schema.to_owned()), &table.name),
        kind,
        columns,
        primary_key,
        indexes,
        foreign_keys,
        definition,
    })
}

fn load_columns(
    connection: &Connection,
    database: &str,
    schema: &str,
    table: &str,
    primary_key: &[String],
) -> Result<Vec<ColumnInfo>, DriverError> {
    let rows = query_all(
        connection,
        COLUMNS_SQL,
        params![database, schema, table],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        },
    )?;
    Ok(rows
        .into_iter()
        .enumerate()
        .map(
            |(index, (name, data_type, nullable, default_value, comment))| {
                let is_auto_increment = default_value
                    .as_deref()
                    .is_some_and(|default| default.starts_with("nextval("));
                ColumnInfo {
                    is_primary_key: primary_key.contains(&name),
                    name,
                    data_type,
                    nullable,
                    default_value,
                    ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
                    is_auto_increment,
                    comment: comment.filter(|comment| !comment.is_empty()),
                }
            },
        )
        .collect())
}

fn load_constraints(
    connection: &Connection,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<Vec<Constraint>, DriverError> {
    query_all(
        connection,
        CONSTRAINTS_SQL,
        params![database, schema, table],
        |row| {
            Ok(Constraint {
                kind: row.get(0)?,
                name: row.get(1)?,
                columns: text_list(row.get_ref(2)?),
                referenced_table: row.get(3)?,
                referenced_columns: text_list(row.get_ref(4)?),
            })
        },
    )
}

fn load_indexes(
    connection: &Connection,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<Vec<IndexInfo>, DriverError> {
    query_all(
        connection,
        INDEXES_SQL,
        params![database, schema, table],
        |row| {
            Ok(IndexInfo {
                name: row.get(0)?,
                is_unique: row.get(1)?,
                is_primary: row.get(2)?,
                columns: text_list(row.get_ref(3)?),
            })
        },
    )
}

fn constraint_indexes(constraints: &[Constraint]) -> Vec<IndexInfo> {
    constraints
        .iter()
        .filter(|constraint| constraint.kind != "FOREIGN KEY")
        .map(|constraint| IndexInfo {
            name: constraint.name.clone(),
            columns: constraint.columns.clone(),
            is_unique: true,
            is_primary: constraint.kind == "PRIMARY KEY",
        })
        .collect()
}

fn foreign_key(constraint: &Constraint, database: &str, schema: &str) -> Option<ForeignKeyInfo> {
    if constraint.kind != "FOREIGN KEY" {
        return None;
    }
    Some(ForeignKeyInfo {
        name: constraint.name.clone(),
        columns: constraint.columns.clone(),
        referenced_table: TableRef::new(
            Some(database.to_owned()),
            Some(schema.to_owned()),
            constraint.referenced_table.clone()?,
        ),
        referenced_columns: constraint.referenced_columns.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::ObjectKind;

    #[test]
    fn keys_constraints_and_indexes_are_reported() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE SEQUENCE seq; \
                 CREATE TABLE parent (id INTEGER PRIMARY KEY DEFAULT nextval('seq'), \
                   code VARCHAR NOT NULL UNIQUE, note VARCHAR); \
                 COMMENT ON COLUMN parent.note IS 'free text'; \
                 CREATE TABLE child (id INTEGER, parent_id INTEGER REFERENCES parent(id)); \
                 CREATE INDEX child_parent ON child(parent_id, id);",
            )
            .unwrap();
        let parent = TableRef::new(None, None, "parent");
        let details = load_table_details(&connection, &parent).unwrap();
        assert_eq!(details.kind, ObjectKind::Table);
        assert_eq!(details.primary_key, vec!["id"]);
        assert!(details.columns[0].is_primary_key && details.columns[0].is_auto_increment);
        assert!(!details.columns[1].nullable);
        assert_eq!(details.columns[2].comment.as_deref(), Some("free text"));
        assert!(
            details
                .indexes
                .iter()
                .any(|index| index.is_unique && !index.is_primary && index.columns == ["code"])
        );
        assert!(
            details
                .definition
                .unwrap()
                .starts_with("CREATE TABLE parent")
        );
        let child = load_table_details(&connection, &TableRef::new(None, None, "child")).unwrap();
        assert_eq!(child.foreign_keys[0].columns, ["parent_id"]);
        assert_eq!(child.foreign_keys[0].referenced_table.name, "parent");
        assert_eq!(child.foreign_keys[0].referenced_columns, ["id"]);
        let index = child
            .indexes
            .iter()
            .find(|index| index.name == "child_parent")
            .unwrap();
        assert_eq!(index.columns, ["parent_id", "id"]);
    }
}
