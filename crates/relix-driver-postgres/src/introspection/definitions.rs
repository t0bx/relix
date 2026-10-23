/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::introspection::ddl::{SequenceOptions, create_sequence};
use crate::row_reader::read_column;
use relix_core::{ObjectKind, TableRef};
use relix_driver_api::{DriverError, SqlBuilder};
use tokio_postgres::Client;

const ROUTINES_SQL: &str = "SELECT pg_get_functiondef(p.oid) \
     FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
     WHERE n.nspname = COALESCE($1::text, current_schema()) AND p.proname = $2::text \
       AND p.prokind::text = $3::text \
     ORDER BY p.oid";

const SEQUENCE_SQL: &str = "SELECT format_type(s.seqtypid, NULL), s.seqstart, s.seqincrement, \
            s.seqmin, s.seqmax, s.seqcache, s.seqcycle \
     FROM pg_catalog.pg_sequence s \
     JOIN pg_catalog.pg_class c ON c.oid = s.seqrelid \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE n.nspname = COALESCE($1::text, current_schema()) AND c.relname = $2::text";

pub(crate) async fn routine_definition(
    client: &Client,
    routine: &TableRef,
    kind: ObjectKind,
) -> Result<Option<String>, DriverError> {
    let prokind = if kind == ObjectKind::Procedure {
        "p"
    } else {
        "f"
    };
    let rows = client
        .query(ROUTINES_SQL, &[&routine.schema, &routine.name, &prokind])
        .await
        .map_err(map_error)?;
    let definitions = rows
        .iter()
        .map(|row| read_column::<String>(row, 0).map(|definition| definition.trim_end().to_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((!definitions.is_empty()).then(|| definitions.join("\n\n")))
}

pub(crate) async fn sequence_definition(
    client: &Client,
    builder: &SqlBuilder,
    sequence: &TableRef,
) -> Result<Option<String>, DriverError> {
    let row = client
        .query_opt(SEQUENCE_SQL, &[&sequence.schema, &sequence.name])
        .await
        .map_err(map_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let options = SequenceOptions {
        data_type: read_column(&row, 0)?,
        start: read_column(&row, 1)?,
        increment: read_column(&row, 2)?,
        min_value: read_column(&row, 3)?,
        max_value: read_column(&row, 4)?,
        cache: read_column(&row, 5)?,
        cycle: read_column(&row, 6)?,
    };
    Ok(Some(create_sequence(
        &builder.qualified_table(sequence),
        &options,
    )))
}
