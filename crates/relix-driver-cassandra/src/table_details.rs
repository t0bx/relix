/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::cql_rows::select_rows;
use crate::definitions::describe;
use crate::schema_columns::{SchemaColumn, load_columns, primary_key};
use relix_core::{ColumnInfo, IndexInfo, ObjectKind, TableDetails, TableRef};
use relix_driver_api::DriverError;
use scylla::client::session::Session;
use std::collections::HashMap;

const VIEW_CQL: &str =
    "SELECT view_name FROM system_schema.views WHERE keyspace_name = ? AND view_name = ?";
const INDEXES_CQL: &str = "SELECT index_name, options FROM system_schema.indexes \
     WHERE keyspace_name = ? AND table_name = ?";
const PRIMARY_KEY_INDEX: &str = "PRIMARY KEY";

pub(crate) async fn table_details(
    session: &Session,
    keyspace: &str,
    table: &TableRef,
) -> Result<TableDetails, DriverError> {
    let columns = load_columns(session, keyspace, &table.name).await?;
    if columns.is_empty() {
        return Err(DriverError::query(format!(
            "table or view '{keyspace}.{}' does not exist",
            table.name
        )));
    }
    let kind = object_kind(session, keyspace, &table.name).await?;
    let primary_key = primary_key(&columns);
    let mut indexes = vec![IndexInfo {
        name: PRIMARY_KEY_INDEX.to_owned(),
        columns: primary_key.clone(),
        is_unique: true,
        is_primary: true,
    }];
    indexes.extend(secondary_indexes(session, keyspace, &table.name).await?);
    Ok(TableDetails {
        table: TableRef::new(Some(keyspace.to_owned()), None, table.name.clone()),
        kind,
        columns: columns.iter().enumerate().map(column_info).collect(),
        primary_key,
        indexes,
        foreign_keys: Vec::new(),
        definition: describe(session, keyspace, &table.name, kind).await,
    })
}

pub(crate) async fn object_kind(
    session: &Session,
    keyspace: &str,
    name: &str,
) -> Result<ObjectKind, DriverError> {
    let views: Vec<(String,)> = select_rows(session, VIEW_CQL, (keyspace, name)).await?;
    Ok(if views.is_empty() {
        ObjectKind::Table
    } else {
        ObjectKind::MaterializedView
    })
}

async fn secondary_indexes(
    session: &Session,
    keyspace: &str,
    table: &str,
) -> Result<Vec<IndexInfo>, DriverError> {
    let rows: Vec<(String, Option<HashMap<String, String>>)> =
        select_rows(session, INDEXES_CQL, (keyspace, table)).await?;
    Ok(rows
        .into_iter()
        .map(|(name, options)| IndexInfo {
            name,
            columns: options
                .and_then(|options| options.get("target").cloned())
                .map(|target| vec![target.replace('"', "")])
                .unwrap_or_default(),
            is_unique: false,
            is_primary: false,
        })
        .collect())
}

fn column_info((index, column): (usize, &SchemaColumn)) -> ColumnInfo {
    let primary = column.kind.is_primary_key();
    ColumnInfo {
        name: column.name.clone(),
        data_type: column.data_type.clone(),
        nullable: !primary,
        default_value: None,
        ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
        is_primary_key: primary,
        is_auto_increment: false,
        comment: Some(column.kind.describe()),
    }
}
