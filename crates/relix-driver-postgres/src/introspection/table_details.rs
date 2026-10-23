/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::flavor::Flavor;
use crate::introspection::ddl::{
    ColumnDefinition, Identity, NamedDefinition, TableDdl, create_table, create_view,
};
use crate::introspection::native_ddl::show_create;
use crate::introspection::relation_sql::{
    COLUMNS_SQL, CONSTRAINTS_SQL, FOREIGN_KEYS_SQL, GENERATED_KEY_DEFAULTS, INDEXES_SQL,
    RELATION_SQL, VIEW_QUERY_SQL, VISIBLE_COLUMNS_SQL,
};
use crate::introspection::timescale;
use crate::row_reader::read_column;
use relix_core::{ColumnInfo, ForeignKeyInfo, IndexInfo, ObjectKind, TableDetails, TableRef};
use relix_driver_api::{DriverError, SqlBuilder};
use tokio_postgres::Client;

pub(crate) struct Relation {
    pub(crate) oid: u32,
    pub(crate) kind: ObjectKind,
    pub(crate) table: TableRef,
    comment: Option<String>,
    partition_key: Option<String>,
}

struct IndexDetails {
    info: IndexInfo,
    definition: Option<String>,
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
    flavor: Flavor,
    database: &str,
    table: &TableRef,
) -> Result<TableDetails, DriverError> {
    let relation = find_relation(client, database, table)
        .await?
        .ok_or_else(|| DriverError::query(format!("relation {table} does not exist")))?;
    let indexes = load_indexes(client, relation.oid).await?;
    let mut columns = load_columns(client, flavor, relation.oid).await?;
    let primary_key = visible_primary_key(&indexes, &columns);
    for column in &mut columns {
        column.info.is_primary_key = primary_key.contains(&column.info.name);
    }
    let foreign_keys = load_foreign_keys(client, database, relation.oid).await?;
    let qualified_name = builder.qualified_table(&relation.table);
    let (kind, definition) = match (flavor, relation.kind) {
        (Flavor::CockroachDb, kind) => (kind, show_create(client, &qualified_name).await?),
        (_, ObjectKind::Table) => {
            let constraints = load_constraints(client, relation.oid).await?;
            let standalone_indexes: Vec<String> = indexes
                .iter()
                .filter(|index| !index.backs_constraint)
                .filter_map(|index| index.definition.clone())
                .collect();
            let mut ddl = create_table(&TableDdl {
                qualified_name: &qualified_name,
                columns: &columns,
                constraints: &constraints,
                standalone_indexes: &standalone_indexes,
                partition_key: relation.partition_key.as_deref(),
                comment: relation.comment.as_deref(),
            });
            if flavor.may_have_timescale_catalog()
                && let Some(hypertable) =
                    timescale::hypertable_statements(client, &qualified_name, &relation.table)
                        .await?
            {
                ddl.push_str(&format!("\n{hypertable}\n"));
            }
            (ObjectKind::Table, Some(ddl))
        }
        _ => {
            let (kind, ddl) = view_definition(client, flavor, &relation, &qualified_name).await?;
            (kind, Some(ddl))
        }
    };
    Ok(TableDetails {
        table: relation.table,
        kind,
        columns: columns.into_iter().map(|column| column.info).collect(),
        primary_key,
        indexes: indexes.into_iter().map(|index| index.info).collect(),
        foreign_keys,
        definition,
    })
}

/// The primary key as visible to `SELECT *`: a key over hidden columns (CockroachDB's
/// implicit `rowid`) cannot address rows read by the data grid, so it is dropped.
fn visible_primary_key(indexes: &[IndexDetails], columns: &[ColumnDefinition]) -> Vec<String> {
    let primary_key = indexes
        .iter()
        .find(|index| index.info.is_primary)
        .map(|index| index.info.columns.clone())
        .unwrap_or_default();
    let visible = primary_key
        .iter()
        .all(|name| columns.iter().any(|column| &column.info.name == name));
    if visible { primary_key } else { Vec::new() }
}

/// Recreates a view-like relation. TimescaleDB continuous aggregates are plain views
/// in `pg_class`; they are reported as materialized views with their own statement.
pub(crate) async fn view_definition(
    client: &Client,
    flavor: Flavor,
    relation: &Relation,
    qualified_name: &str,
) -> Result<(ObjectKind, String), DriverError> {
    if flavor.may_have_timescale_catalog()
        && relation.kind == ObjectKind::View
        && let Some(ddl) =
            timescale::continuous_aggregate_definition(client, qualified_name, &relation.table)
                .await?
    {
        return Ok((ObjectKind::MaterializedView, ddl));
    }
    Ok((
        relation.kind,
        view_ddl(client, relation, qualified_name).await?,
    ))
}

async fn view_ddl(
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

async fn load_columns(
    client: &Client,
    flavor: Flavor,
    oid: u32,
) -> Result<Vec<ColumnDefinition>, DriverError> {
    let sql = match flavor {
        Flavor::CockroachDb => VISIBLE_COLUMNS_SQL,
        Flavor::Postgres | Flavor::YugabyteDb | Flavor::TimescaleDb => COLUMNS_SQL,
    };
    let rows = client.query(sql, &[&oid]).await.map_err(map_error)?;
    rows.iter()
        .map(|row| {
            let default_value: Option<String> = read_column(row, 3)?;
            let identity = Identity::from_code(&read_column::<String>(row, 5)?);
            let ordinal: i32 = read_column(row, 4)?;
            let is_serial = default_value.as_deref().is_some_and(|default| {
                GENERATED_KEY_DEFAULTS
                    .iter()
                    .any(|generator| default.starts_with(generator))
            });
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
