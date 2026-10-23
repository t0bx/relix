/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::catalog;
use crate::api::models::{TableConstraints, TableField};
use crate::connection::BigQueryConnection;
use crate::field_types::type_label;
use crate::introspection::objects::table_kind;
use relix_core::{ColumnInfo, ForeignKeyInfo, TableDetails, TableRef};
use relix_driver_api::DriverError;

/// Describes a table from `tables.get`: schema fields, the (unenforced) primary and
/// foreign key constraints and, for views, their query.
pub(crate) async fn describe(
    connection: &BigQueryConnection,
    table: &TableRef,
) -> Result<TableDetails, DriverError> {
    let (project, dataset) = connection.table_scope(table)?;
    let metadata = catalog::table(connection.client(), &project, &dataset, &table.name).await?;
    let constraints = metadata.table_constraints.unwrap_or_default();
    let primary_key = constraints
        .primary_key
        .as_ref()
        .map(|key| key.columns.clone())
        .unwrap_or_default();
    let fields = metadata.schema.unwrap_or_default().fields;
    Ok(TableDetails {
        table: table.clone(),
        kind: table_kind(metadata.table_type.as_deref()),
        columns: fields
            .iter()
            .enumerate()
            .map(|(index, field)| column(index, field, &primary_key))
            .collect(),
        primary_key,
        indexes: Vec::new(),
        foreign_keys: foreign_keys(constraints),
        definition: metadata
            .view
            .or(metadata.materialized_view)
            .and_then(|view| view.query),
    })
}

fn column(index: usize, field: &TableField, primary_key: &[String]) -> ColumnInfo {
    ColumnInfo {
        name: field.name.clone(),
        data_type: type_label(field),
        nullable: !field.is_required(),
        default_value: field.default_value_expression.clone(),
        ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
        is_primary_key: primary_key.contains(&field.name),
        is_auto_increment: false,
        comment: field.description.clone(),
    }
}

fn foreign_keys(constraints: TableConstraints) -> Vec<ForeignKeyInfo> {
    constraints
        .foreign_keys
        .into_iter()
        .map(|key| {
            let referenced = key.referenced_table;
            ForeignKeyInfo {
                name: key.name.unwrap_or_default(),
                columns: key
                    .column_references
                    .iter()
                    .map(|pair| pair.referencing_column.clone())
                    .collect(),
                referenced_table: TableRef::new(
                    Some(referenced.project_id),
                    Some(referenced.dataset_id),
                    referenced.table_id,
                ),
                referenced_columns: key
                    .column_references
                    .into_iter()
                    .map(|pair| pair.referenced_column)
                    .collect(),
            }
        })
        .collect()
}
