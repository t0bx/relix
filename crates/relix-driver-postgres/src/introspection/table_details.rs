/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::introspection::ddl::{
    ColumnDefinition, Identity, NamedDefinition, TableDdl, create_table, create_view,
};
use crate::row_reader::read_column;
use relix_core::{ColumnInfo, ForeignKeyInfo, IndexInfo, ObjectKind, TableDetails, TableRef};
use relix_driver_api::{DriverError, SqlBuilder};
use tokio_postgres::Client;

const RELATION_SQL: &str = "SELECT c.oid, c.relkind::text, n.nspname::text, c.relname::text, \
            obj_description(c.oid, 'pg_class'), \
            CASE WHEN c.relkind = 'p' THEN pg_get_partkeydef(c.oid) END \
     FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE c.relname = $2::text AND n.nspname = COALESCE($1::text, current_schema()) \
       AND c.relkind IN ('r', 'p', 'f', 'v', 'm')";

const COLUMNS_SQL: &str = "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod), \
            NOT a.attnotnull, pg_get_expr(d.adbin, d.adrelid), a.attnum::int4, \
            a.attidentity::text, a.attgenerated::text, col_description(a.attrelid, a.attnum) \
     FROM pg_catalog.pg_attribute a \
     LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
     WHERE a.attrelid = $1 AND a.attnum > 0 AND NOT a.attisdropped \
     ORDER BY a.attnum";

const INDEXES_SQL: &str = "SELECT i.relname::text, ix.indisunique, ix.indisprimary, \
            ARRAY(SELECT CASE WHEN k.attnum = 0 \
                              THEN pg_get_indexdef(ix.indexrelid, k.ord::int4, true) \
                              ELSE a.attname::text END \
                  FROM unnest(ix.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) \
                  LEFT JOIN pg_catalog.pg_attribute a \
                         ON a.attrelid = ix.indrelid AND a.attnum = k.attnum \
                  WHERE k.ord <= ix.indnkeyatts ORDER BY k.ord), \
            pg_get_indexdef(ix.indexrelid), \
            EXISTS (SELECT 1 FROM pg_catalog.pg_constraint con \
                    WHERE con.conindid = ix.indexrelid AND con.conrelid = ix.indrelid) \
     FROM pg_catalog.pg_index ix JOIN pg_catalog.pg_class i ON i.oid = ix.indexrelid \
     WHERE ix.indrelid = $1 \
     ORDER BY ix.indisprimary DESC, i.relname";

const FOREIGN_KEYS_SQL: &str = "SELECT con.conname::text, \
            ARRAY(SELECT a.attname::text \
                  FROM unnest(con.conkey) WITH ORDINALITY AS k(attnum, ord) \
                  JOIN pg_catalog.pg_attribute a \
                    ON a.attrelid = con.conrelid AND a.attnum = k.attnum \
                  ORDER BY k.ord), \
            rn.nspname::text, rc.relname::text, \
            ARRAY(SELECT a.attname::text \
                  FROM unnest(con.confkey) WITH ORDINALITY AS k(attnum, ord) \
                  JOIN pg_catalog.pg_attribute a \
                    ON a.attrelid = con.confrelid AND a.attnum = k.attnum \
                  ORDER BY k.ord) \
     FROM pg_catalog.pg_constraint con \
     JOIN pg_catalog.pg_class rc ON rc.oid = con.confrelid \
     JOIN pg_catalog.pg_namespace rn ON rn.oid = rc.relnamespace \
     WHERE con.conrelid = $1 AND con.contype = 'f' \
     ORDER BY con.conname";

const CONSTRAINTS_SQL: &str = "SELECT conname::text, pg_get_constraintdef(oid, true) \
     FROM pg_catalog.pg_constraint \
     WHERE conrelid = $1 AND contype IN ('p', 'u', 'c', 'f', 'x') \
     ORDER BY CASE contype WHEN 'p' THEN 0 WHEN 'u' THEN 1 WHEN 'c' THEN 2 \
                           WHEN 'f' THEN 3 ELSE 4 END, conname";

const VIEW_QUERY_SQL: &str = "SELECT pg_get_viewdef($1::oid, true)";

pub(crate) struct Relation {
    pub(crate) oid: u32,
    pub(crate) kind: ObjectKind,
    pub(crate) table: TableRef,
    comment: Option<String>,
    partition_key: Option<String>,
}

struct IndexDetails {
    info: IndexInfo,
    definition: String,
    backs_constraint: bool,
}

pub(crate) async fn find_relation(
    client: &Client,
    database: &str,
    table: &TableRef,
) -> Result<Option<Relation>, DriverError> {
    let row = client
        .query_opt(RELATION_SQL, &[&table.schema, &table.name])
        .await
        .map_err(map_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let kind = match read_column::<String>(&row, 1)?.as_str() {
        "v" => ObjectKind::View,
        "m" => ObjectKind::MaterializedView,
        _ => ObjectKind::Table,
    };
    Ok(Some(Relation {
        oid: read_column(&row, 0)?,
        kind,
        table: TableRef::new(
            Some(database.to_owned()),
            Some(read_column(&row, 2)?),
            read_column::<String>(&row, 3)?,
        ),
        comment: read_column(&row, 4)?,
        partition_key: read_column(&row, 5)?,
    }))
}

pub(crate) async fn load_table_details(
    client: &Client,
    builder: &SqlBuilder,
    database: &str,
    table: &TableRef,
) -> Result<TableDetails, DriverError> {
    let relation = find_relation(client, database, table)
        .await?
        .ok_or_else(|| DriverError::query(format!("relation {table} does not exist")))?;
    let indexes = load_indexes(client, relation.oid).await?;
    let primary_key = indexes
        .iter()
        .find(|index| index.info.is_primary)
        .map(|index| index.info.columns.clone())
        .unwrap_or_default();
    let mut columns = load_columns(client, relation.oid).await?;
    for column in &mut columns {
        column.info.is_primary_key = primary_key.contains(&column.info.name);
    }
    let foreign_keys = load_foreign_keys(client, database, relation.oid).await?;
    let qualified_name = builder.qualified_table(&relation.table);
    let definition = match relation.kind {
        ObjectKind::Table => {
            let constraints = load_constraints(client, relation.oid).await?;
            let standalone_indexes: Vec<String> = indexes
                .iter()
                .filter(|index| !index.backs_constraint)
                .map(|index| index.definition.clone())
                .collect();
            create_table(&TableDdl {
                qualified_name: &qualified_name,
                columns: &columns,
                constraints: &constraints,
                standalone_indexes: &standalone_indexes,
                partition_key: relation.partition_key.as_deref(),
                comment: relation.comment.as_deref(),
            })
        }
        _ => view_ddl(client, &relation, &qualified_name).await?,
    };
    Ok(TableDetails {
        table: relation.table,
        kind: relation.kind,
        columns: columns.into_iter().map(|column| column.info).collect(),
        primary_key,
        indexes: indexes.into_iter().map(|index| index.info).collect(),
        foreign_keys,
        definition: Some(definition),
    })
}

pub(crate) async fn view_ddl(
    client: &Client,
    relation: &Relation,
    qualified_name: &str,
) -> Result<String, DriverError> {
    let row = client
        .query_one(VIEW_QUERY_SQL, &[&relation.oid])
        .await
        .map_err(map_error)?;
    let query: Option<String> = read_column(&row, 0)?;
    Ok(create_view(
        qualified_name,
        relation.kind == ObjectKind::MaterializedView,
        query.as_deref().unwrap_or_default(),
    ))
}

async fn load_columns(client: &Client, oid: u32) -> Result<Vec<ColumnDefinition>, DriverError> {
    let rows = client
        .query(COLUMNS_SQL, &[&oid])
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|row| {
            let default_value: Option<String> = read_column(row, 3)?;
            let identity = Identity::from_code(&read_column::<String>(row, 5)?);
            let ordinal: i32 = read_column(row, 4)?;
            let is_serial = default_value
                .as_deref()
                .is_some_and(|default| default.starts_with("nextval("));
            Ok(ColumnDefinition {
                info: ColumnInfo {
                    name: read_column(row, 0)?,
                    data_type: read_column(row, 1)?,
                    nullable: read_column(row, 2)?,
                    default_value,
                    ordinal: u32::try_from(ordinal).unwrap_or_default(),
                    is_primary_key: false,
                    is_auto_increment: identity != Identity::None || is_serial,
                    comment: read_column(row, 7)?,
                },
                identity,
                generated: !read_column::<String>(row, 6)?.is_empty(),
            })
        })
        .collect()
}

async fn load_indexes(client: &Client, oid: u32) -> Result<Vec<IndexDetails>, DriverError> {
    let rows = client
        .query(INDEXES_SQL, &[&oid])
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|row| {
            Ok(IndexDetails {
                info: IndexInfo {
                    name: read_column(row, 0)?,
                    is_unique: read_column(row, 1)?,
                    is_primary: read_column(row, 2)?,
                    columns: read_column(row, 3)?,
                },
                definition: read_column(row, 4)?,
                backs_constraint: read_column(row, 5)?,
            })
        })
        .collect()
}

async fn load_foreign_keys(
    client: &Client,
    database: &str,
    oid: u32,
) -> Result<Vec<ForeignKeyInfo>, DriverError> {
    let rows = client
        .query(FOREIGN_KEYS_SQL, &[&oid])
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|row| {
            Ok(ForeignKeyInfo {
                name: read_column(row, 0)?,
                columns: read_column(row, 1)?,
                referenced_table: TableRef::new(
                    Some(database.to_owned()),
                    Some(read_column(row, 2)?),
                    read_column::<String>(row, 3)?,
                ),
                referenced_columns: read_column(row, 4)?,
            })
        })
        .collect()
}

async fn load_constraints(client: &Client, oid: u32) -> Result<Vec<NamedDefinition>, DriverError> {
    let rows = client
        .query(CONSTRAINTS_SQL, &[&oid])
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|row| {
            Ok(NamedDefinition {
                name: read_column(row, 0)?,
                definition: read_column(row, 1)?,
            })
        })
        .collect()
}
