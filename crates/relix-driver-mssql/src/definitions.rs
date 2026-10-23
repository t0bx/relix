/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog_rows::{flag_at, in_database, int_at, query_rows, text_at};
use crate::connection::sql_builder;
use crate::connector::MsSqlClient;
use crate::ddl::{SequenceDefinition, create_sequence, create_table};
use crate::table_structure::table_structure;
use relix_core::{ObjectKind, TableRef};
use relix_driver_api::DriverError;

const MODULE_SQL: &str = "SELECT m.definition FROM {db}.sys.sql_modules m \
     WHERE m.object_id = OBJECT_ID(@P1)";

const SEQUENCE_SQL: &str = "SELECT t.name, CAST(s.start_value AS nvarchar(64)), \
       CAST(s.increment AS nvarchar(64)), CAST(s.minimum_value AS nvarchar(64)), \
       CAST(s.maximum_value AS nvarchar(64)), s.is_cycling, s.is_cached, \
       CAST(s.cache_size AS int) \
     FROM {db}.sys.sequences s JOIN {db}.sys.types t ON t.user_type_id = s.user_type_id \
     WHERE s.object_id = OBJECT_ID(@P1)";

/// Returns the source of views and routines (`NULL` for encrypted modules), and generated
/// DDL for tables and sequences. `object` must carry database and schema.
pub async fn object_definition(
    client: &mut MsSqlClient,
    kind: ObjectKind,
    object: &TableRef,
) -> Result<Option<String>, DriverError> {
    let object_name = sql_builder().qualified_table(object);
    match kind {
        ObjectKind::Table => {
            let structure = table_structure(client, object, &object_name).await?;
            Ok(Some(create_table(object, &structure)))
        }
        ObjectKind::Sequence => sequence_definition(client, object, &object_name).await,
        ObjectKind::View
        | ObjectKind::MaterializedView
        | ObjectKind::Function
        | ObjectKind::Procedure
        | ObjectKind::Trigger => module_definition(client, object, &object_name).await,
        ObjectKind::NodeLabel | ObjectKind::RelationshipType => Ok(None),
    }
}

pub async fn module_definition(
    client: &mut MsSqlClient,
    object: &TableRef,
    object_name: &str,
) -> Result<Option<String>, DriverError> {
    let database = object.database.as_deref().unwrap_or_default();
    let rows = query_rows(client, &in_database(MODULE_SQL, database), &[&object_name]).await?;
    Ok(rows.first().and_then(|row| text_at(row, 0)))
}

async fn sequence_definition(
    client: &mut MsSqlClient,
    object: &TableRef,
    object_name: &str,
) -> Result<Option<String>, DriverError> {
    let database = object.database.as_deref().unwrap_or_default();
    let rows = query_rows(
        client,
        &in_database(SEQUENCE_SQL, database),
        &[&object_name],
    )
    .await?;
    Ok(rows.first().and_then(|row| {
        let definition = SequenceDefinition {
            type_name: text_at(row, 0)?,
            start: text_at(row, 1)?,
            increment: text_at(row, 2)?,
            minimum: text_at(row, 3)?,
            maximum: text_at(row, 4)?,
            cycling: flag_at(row, 5),
            cache: flag_at(row, 6).then(|| int_at(row, 7)),
        };
        Some(create_sequence(object, &definition))
    }))
}
