/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::connection::Neo4jConnection;
use crate::cypher::{fetch_all, open_stream};
use crate::errors::map_error;
use crate::graph_object::GraphObject;
use crate::page_statements::{PROPERTIES_ALIAS, count_query, key_sample_query, page_query};
use crate::property_edits::apply_change_set;
use crate::row_reader::{bolt_value, optional_integer, read_rows, text_list};
use crate::values::{to_value, type_label};
use async_trait::async_trait;
use neo4rs::{BoltMap, BoltString, BoltType, Row};
use relix_core::{ChangeReport, ChangeSet, ColumnMeta, RowBatch, TableQuery, TableRef, Value};
use relix_driver_api::{DriverError, TableDataAccess};
use std::collections::BTreeSet;

#[async_trait]
impl TableDataAccess for Neo4jConnection {
    /// Reads one page of nodes or relationships. Property columns are the keys found on
    /// the first thousand members plus those on the page itself, sorted by name, so the
    /// columns stay stable while paging through a label with a regular shape.
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        let live = self.live().await?;
        let database = self.database_name(table.database.as_deref());
        let object = GraphObject::resolve(&live.graph, database, table).await?;
        let mut keys: BTreeSet<String> = BTreeSet::new();
        for row in fetch_all(&live.graph, Some(database), key_sample_query(&object)).await? {
            keys.extend(text_list(&row, "keys")?);
        }
        let stream = open_stream(&live.graph, Some(database), page_query(&object, query))
            .await
            .map_err(map_error)?;
        let (rows, has_more) = read_rows(stream, query.limit as usize)
            .await
            .map_err(map_error)?;
        let mut properties = Vec::with_capacity(rows.len());
        for row in &rows {
            let map = property_map(row)?;
            keys.extend(map.value.keys().map(|key| key.value.clone()));
            properties.push(map);
        }
        page_batch(&object, &rows, &properties, keys, has_more)
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        let live = self.live().await?;
        let database = self.database_name(table.database.as_deref());
        let object = GraphObject::resolve(&live.graph, database, table).await?;
        let rows = fetch_all(&live.graph, Some(database), count_query(&object, filter)).await?;
        let count = match rows.first() {
            Some(row) => optional_integer(row, "count")?.unwrap_or_default(),
            None => 0,
        };
        Ok(u64::try_from(count).unwrap_or_default())
    }

    async fn apply_changes(&self, changes: &ChangeSet) -> Result<ChangeReport, DriverError> {
        if changes.is_empty() {
            return Ok(ChangeReport::default());
        }
        let live = self.live().await?;
        let database = self.database_name(changes.table.database.as_deref());
        let object = GraphObject::resolve(&live.graph, database, &changes.table).await?;
        apply_change_set(&live.graph, database, object.entity, &changes.changes).await
    }
}

fn property_map(row: &Row) -> Result<BoltMap, DriverError> {
    Ok(match bolt_value(row, PROPERTIES_ALIAS)? {
        BoltType::Map(map) => map,
        _ => BoltMap::new(),
    })
}

fn page_batch(
    object: &GraphObject,
    rows: &[Row],
    properties: &[BoltMap],
    keys: BTreeSet<String>,
    has_more: bool,
) -> Result<RowBatch, DriverError> {
    let fixed = object.entity.fixed_columns();
    let keys: Vec<String> = keys
        .into_iter()
        .filter(|key| !object.entity.is_fixed_column(key))
        .collect();
    let mut columns: Vec<ColumnMeta> = fixed
        .iter()
        .map(|(name, _)| {
            let type_name = if *name == "labels" {
                "List<String>"
            } else {
                "String"
            };
            ColumnMeta::new(*name, Some(type_name.to_owned()))
        })
        .collect();
    let mut property_types: Vec<Option<String>> = vec![None; keys.len()];
    let mut values = Vec::with_capacity(rows.len());
    for (row, map) in rows.iter().zip(properties) {
        let mut converted: Vec<Value> = Vec::with_capacity(fixed.len() + keys.len());
        for (name, _) in fixed {
            converted.push(to_value(&bolt_value(row, name)?));
        }
        for (index, key) in keys.iter().enumerate() {
            match map.value.get(&BoltString::new(key)) {
                Some(value) => {
                    if property_types[index].is_none() {
                        property_types[index] = Some(type_label(value).to_owned());
                    }
                    converted.push(to_value(value));
                }
                None => converted.push(Value::Null),
            }
        }
        values.push(converted);
    }
    columns.extend(
        keys.into_iter()
            .zip(property_types)
            .map(|(key, type_name)| ColumnMeta::new(key, type_name)),
    );
    Ok(RowBatch {
        columns,
        rows: values,
        has_more,
    })
}
