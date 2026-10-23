/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::{database_of, master_entry};
use crate::query::query_all;
use relix_core::{ColumnInfo, ForeignKeyInfo, IndexInfo, ObjectKind, TableDetails, TableRef};
use relix_driver_api::DriverError;
use rusqlite::{Connection, params};

const RELATION_KINDS: [ObjectKind; 2] = [ObjectKind::Table, ObjectKind::View];
const VIRTUAL_TABLE_HIDDEN_COLUMN: i64 = 1;
const PRIMARY_KEY_ORIGIN: &str = "pk";
const EXPRESSION_COLUMN: &str = "<expression>";

struct RawColumn {
    name: String,
    data_type: String,
    not_null: bool,
    default_value: Option<String>,
    primary_key_position: i64,
}

struct ForeignKeyPart {
    id: i64,
    table: String,
    from: String,
    to: Option<String>,
}

pub(crate) fn load_table_details(
    connection: &Connection,
    table: &TableRef,
) -> Result<TableDetails, DriverError> {
    let database = database_of(table);
    let entry = master_entry(connection, database, &table.name, &RELATION_KINDS)?
        .ok_or_else(|| DriverError::query(format!("table {database}.{} not found", table.name)))?;
    let raw_columns = read_columns(connection, database, &table.name)?;
    let indexes = read_indexes(connection, database, &table.name)?;
    let primary_key = primary_key_columns(&raw_columns);
    let rowid_alias = rowid_alias(entry.kind, &raw_columns, &primary_key, &indexes);
    let columns = raw_columns
        .into_iter()
        .enumerate()
        .map(|(index, column)| {
            let is_rowid_alias = rowid_alias.as_deref() == Some(column.name.as_str());
            ColumnInfo {
                nullable: !column.not_null && !is_rowid_alias,
                ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
                is_primary_key: column.primary_key_position > 0,
                is_auto_increment: is_rowid_alias,
                comment: None,
                name: column.name,
                data_type: column.data_type,
                default_value: column.default_value,
            }
        })
        .collect();
    Ok(TableDetails {
        table: TableRef::new(Some(database.to_owned()), None, table.name.clone()),
        kind: entry.kind,
        columns,
        primary_key,
        foreign_keys: read_foreign_keys(connection, database, &table.name)?,
        indexes,
        definition: entry.sql,
    })
}

fn read_columns(
    connection: &Connection,
    database: &str,
    table: &str,
) -> Result<Vec<RawColumn>, DriverError> {
    let columns = query_all(
        connection,
        "SELECT name, type, \"notnull\", dflt_value, pk, hidden \
         FROM pragma_table_xinfo(?1, ?2) ORDER BY cid",
        params![table, database],
        |row| {
            Ok((
                RawColumn {
                    name: row.get(0)?,
                    data_type: row.get(1)?,
                    not_null: row.get(2)?,
                    default_value: row.get(3)?,
                    primary_key_position: row.get(4)?,
                },
                row.get::<_, i64>(5)?,
            ))
        },
    )?;
    Ok(columns
        .into_iter()
        .filter(|(_, hidden)| *hidden != VIRTUAL_TABLE_HIDDEN_COLUMN)
        .map(|(column, _)| column)
        .collect())
}

fn primary_key_columns(columns: &[RawColumn]) -> Vec<String> {
    let mut keyed: Vec<&RawColumn> = columns
        .iter()
        .filter(|column| column.primary_key_position > 0)
        .collect();
    keyed.sort_by_key(|column| column.primary_key_position);
    keyed
        .into_iter()
        .map(|column| column.name.clone())
        .collect()
}

/// Returns the column that aliases the rowid: a single-column `INTEGER PRIMARY KEY` of a
/// rowid table. Such a key has no backing `pk` index, whereas other primary keys and
/// `WITHOUT ROWID` tables do, which is how the two cases are told apart.
fn rowid_alias(
    kind: ObjectKind,
    columns: &[RawColumn],
    primary_key: &[String],
    indexes: &[IndexInfo],
) -> Option<String> {
    if kind != ObjectKind::Table || indexes.iter().any(|index| index.is_primary) {
        return None;
    }
    let [key] = primary_key else {
        return None;
    };
    columns
        .iter()
        .find(|column| &column.name == key)
        .filter(|column| column.data_type.eq_ignore_ascii_case("INTEGER"))
        .map(|column| column.name.clone())
}

fn read_indexes(
    connection: &Connection,
    database: &str,
    table: &str,
) -> Result<Vec<IndexInfo>, DriverError> {
    let listed = query_all(
        connection,
        "SELECT name, \"unique\", origin FROM pragma_index_list(?1, ?2) ORDER BY name",
        params![table, database],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    listed
        .into_iter()
        .map(|(name, is_unique, origin)| {
            let columns = query_all(
                connection,
                "SELECT name FROM pragma_index_info(?1, ?2) ORDER BY seqno",
                params![name, database],
                |row| row.get::<_, Option<String>>(0),
            )?
            .into_iter()
            .map(|column| column.unwrap_or_else(|| EXPRESSION_COLUMN.to_owned()))
            .collect();
            Ok(IndexInfo {
                name,
                columns,
                is_unique,
                is_primary: origin == PRIMARY_KEY_ORIGIN,
            })
        })
        .collect()
}

fn read_foreign_keys(
    connection: &Connection,
    database: &str,
    table: &str,
) -> Result<Vec<ForeignKeyInfo>, DriverError> {
    let parts = query_all(
        connection,
        "SELECT id, \"table\", \"from\", \"to\" FROM pragma_foreign_key_list(?1, ?2) \
         ORDER BY id, seq",
        params![table, database],
        |row| {
            Ok(ForeignKeyPart {
                id: row.get(0)?,
                table: row.get(1)?,
                from: row.get(2)?,
                to: row.get(3)?,
            })
        },
    )?;
    parts
        .chunk_by(|left, right| left.id == right.id)
        .map(|group| foreign_key_from_parts(connection, database, table, group))
        .collect()
}

fn foreign_key_from_parts(
    connection: &Connection,
    database: &str,
    table: &str,
    parts: &[ForeignKeyPart],
) -> Result<ForeignKeyInfo, DriverError> {
    let first = parts
        .first()
        .ok_or_else(|| DriverError::internal("foreign key without columns"))?;
    let explicit_targets: Option<Vec<String>> = parts.iter().map(|part| part.to.clone()).collect();
    let referenced_columns = match explicit_targets {
        Some(columns) => columns,
        None => primary_key_columns(&read_columns(connection, database, &first.table)?),
    };
    Ok(ForeignKeyInfo {
        name: format!("{table}_fk_{}", first.id),
        columns: parts.iter().map(|part| part.from.clone()).collect(),
        referenced_table: TableRef::new(Some(database.to_owned()), None, first.table.clone()),
        referenced_columns,
    })
}
