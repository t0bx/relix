/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::kind_of_engine;
use crate::client::{ClickHouseClient, QueryRequest};
use crate::definitions::show_create;
use crate::metadata_rows::{flag_at, optional_text_at, text_at};
use crate::values::is_nullable;
use relix_core::{ColumnInfo, IndexInfo, TableDetails, TableRef, Value};
use relix_driver_api::DriverError;

const TABLE_SQL: &str = "SELECT engine, primary_key FROM system.tables \
     WHERE database = {database:String} AND name = {table:String}";
const COLUMNS_SQL: &str = "SELECT name, type, default_kind, default_expression, comment, \
     is_in_primary_key FROM system.columns \
     WHERE database = {database:String} AND table = {table:String} ORDER BY position";
const INDEXES_SQL: &str = "SELECT name, expr FROM system.data_skipping_indices \
     WHERE database = {database:String} AND table = {table:String} ORDER BY name";
const PRIMARY_KEY_INDEX: &str = "PRIMARY KEY";

/// Loads columns, the primary (sparse, non-unique) key and data skipping indexes.
/// ClickHouse has no foreign keys.
pub(crate) async fn load_table_details(
    client: &ClickHouseClient,
    table: &TableRef,
    database: &str,
) -> Result<TableDetails, DriverError> {
    let table_request = QueryRequest::new(TABLE_SQL)
        .param("database", database)
        .param("table", table.name.as_str());
    let info = client.rows(&table_request).await?;
    let info_row = info.rows.first().ok_or_else(|| {
        DriverError::query(format!("table {database}.{} does not exist", table.name))
    })?;
    let engine = text_at(info_row, 0);
    let column_rows = client
        .rows(
            &QueryRequest::new(COLUMNS_SQL)
                .param("database", database)
                .param("table", table.name.as_str()),
        )
        .await?
        .rows;
    let primary_key = primary_key_columns(&text_at(info_row, 1), &column_rows);
    let columns = column_rows
        .iter()
        .enumerate()
        .map(|(index, row)| column(index, row, &primary_key))
        .collect();
    let mut indexes = Vec::new();
    if !primary_key.is_empty() {
        indexes.push(IndexInfo {
            name: PRIMARY_KEY_INDEX.to_owned(),
            columns: primary_key.clone(),
            is_unique: false,
            is_primary: true,
        });
    }
    let index_rows = client
        .rows(
            &QueryRequest::new(INDEXES_SQL)
                .param("database", database)
                .param("table", table.name.as_str()),
        )
        .await?
        .rows;
    indexes.extend(index_rows.iter().map(|row| IndexInfo {
        name: text_at(row, 0),
        columns: vec![text_at(row, 1)],
        is_unique: false,
        is_primary: false,
    }));
    let resolved = TableRef::new(Some(database.to_owned()), None, table.name.clone());
    let definition = show_create(client, &resolved, &engine).await?;
    Ok(TableDetails {
        table: resolved,
        kind: kind_of_engine(&engine),
        columns,
        primary_key,
        indexes,
        foreign_keys: Vec::new(),
        definition,
    })
}

fn column(index: usize, row: &[Value], primary_key: &[String]) -> ColumnInfo {
    let name = text_at(row, 0);
    let data_type = text_at(row, 1);
    let default_kind = text_at(row, 2);
    let default_value = optional_text_at(row, 3).map(|expression| match default_kind.as_str() {
        "" | "DEFAULT" => expression,
        kind => format!("{kind} {expression}"),
    });
    ColumnInfo {
        is_primary_key: primary_key.contains(&name),
        nullable: is_nullable(&data_type),
        name,
        data_type,
        default_value,
        ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
        is_auto_increment: false,
        comment: optional_text_at(row, 4),
    }
}

/// Orders the key columns as written in the table's `PRIMARY KEY` expression; key
/// parts that are expressions rather than columns are skipped.
fn primary_key_columns(expression: &str, columns: &[Vec<Value>]) -> Vec<String> {
    let flagged: Vec<String> = columns
        .iter()
        .filter(|row| flag_at(row, 5))
        .map(|row| text_at(row, 0))
        .collect();
    let ordered: Vec<String> = split_top_level(expression)
        .into_iter()
        .map(|part| part.trim().trim_matches('`').to_owned())
        .filter(|part| flagged.contains(part))
        .collect();
    if ordered.is_empty() { flagged } else { ordered }
}

fn split_top_level(expression: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut quoted = false;
    for ch in expression.chars() {
        match ch {
            '`' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            ',' if !quoted && depth == 0 => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_column(name: &str, in_key: bool) -> Vec<Value> {
        vec![
            Value::Text(name.into()),
            Value::Text("UInt64".into()),
            Value::Text(String::new()),
            Value::Text(String::new()),
            Value::Text(String::new()),
            Value::Int(i64::from(in_key)),
        ]
    }

    #[test]
    fn primary_key_follows_the_key_expression() {
        let columns = vec![
            key_column("a", true),
            key_column("b, c", true),
            key_column("d", false),
        ];
        assert_eq!(
            primary_key_columns("`b, c`, toDate(x), a", &columns),
            vec!["b, c".to_owned(), "a".to_owned()]
        );
        assert_eq!(primary_key_columns("", &columns), vec!["a", "b, c"]);
    }

    #[test]
    fn defaults_keep_their_kind() {
        let mut row = key_column("ts", false);
        row[2] = Value::Text("MATERIALIZED".into());
        row[3] = Value::Text("now()".into());
        let info = column(0, &row, &[]);
        assert_eq!(info.default_value.as_deref(), Some("MATERIALIZED now()"));
        assert_eq!(info.ordinal, 1);
    }
}
