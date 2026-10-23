/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::introspection::ddl::create_continuous_aggregate;
use crate::row_reader::read_column;
use relix_core::{Dialect, ObjectInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;
use std::collections::HashSet;
use tokio_postgres::Client;

const HYPERTABLE_LABEL: &str = "hypertable";
const CONTINUOUS_AGGREGATE_LABEL: &str = "continuous aggregate";

const INSTALLED_SQL: &str = "SELECT to_regclass('timescaledb_information.hypertables') IS NOT NULL";

const HYPERTABLES_SQL: &str = "SELECT hypertable_schema::text, hypertable_name::text \
     FROM timescaledb_information.hypertables \
     WHERE $1::text IS NULL OR hypertable_schema = $1::text";

const CONTINUOUS_AGGREGATES_SQL: &str = "SELECT view_schema::text, view_name::text \
     FROM timescaledb_information.continuous_aggregates \
     WHERE $1::text IS NULL OR view_schema = $1::text";

const DIMENSIONS_SQL: &str = "SELECT column_name::text, dimension_type::text, \
            time_interval::text, integer_interval, num_partitions::int4 \
     FROM timescaledb_information.dimensions \
     WHERE hypertable_schema = COALESCE($1::text, current_schema()) \
       AND hypertable_name = $2::text \
     ORDER BY dimension_number";

const CONTINUOUS_AGGREGATE_SQL: &str = "SELECT view_definition \
     FROM timescaledb_information.continuous_aggregates \
     WHERE view_schema = COALESCE($1::text, current_schema()) AND view_name = $2::text";

type QualifiedName = (String, String);

#[derive(Debug, Default)]
pub(crate) struct TimescaleCatalog {
    hypertables: HashSet<QualifiedName>,
    continuous_aggregates: HashSet<QualifiedName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Dimension {
    column: String,
    is_time: bool,
    time_interval: Option<String>,
    integer_interval: Option<i64>,
    partitions: Option<i32>,
}

/// Loads hypertables and continuous aggregates of the connected database, or `None`
/// when the `timescaledb` extension is not installed there.
pub(crate) async fn load_catalog(
    client: &Client,
    schema: Option<&str>,
) -> Result<Option<TimescaleCatalog>, DriverError> {
    if !is_installed(client).await? {
        return Ok(None);
    }
    Ok(Some(TimescaleCatalog {
        hypertables: qualified_names(client, HYPERTABLES_SQL, schema).await?,
        continuous_aggregates: qualified_names(client, CONTINUOUS_AGGREGATES_SQL, schema).await?,
    }))
}

impl TimescaleCatalog {
    pub(crate) fn annotate(&self, objects: &mut [ObjectInfo]) {
        for object in objects {
            let key = (
                object.object.schema.clone().unwrap_or_default(),
                object.object.name.clone(),
            );
            if object.kind == ObjectKind::Table && self.hypertables.contains(&key) {
                object.comment = Some(labelled(HYPERTABLE_LABEL, object.comment.take()));
            } else if object.kind == ObjectKind::View && self.continuous_aggregates.contains(&key) {
                object.kind = ObjectKind::MaterializedView;
                object.comment = Some(labelled(CONTINUOUS_AGGREGATE_LABEL, object.comment.take()));
            }
        }
    }
}

/// Returns the `create_hypertable`/`add_dimension` calls that recreate the
/// partitioning of a hypertable, or `None` for regular tables.
pub(crate) async fn hypertable_statements(
    client: &Client,
    qualified_name: &str,
    table: &TableRef,
) -> Result<Option<String>, DriverError> {
    if !is_installed(client).await? {
        return Ok(None);
    }
    let rows = client
        .query(DIMENSIONS_SQL, &[&table.schema, &table.name])
        .await
        .map_err(map_error)?;
    let dimensions = rows
        .iter()
        .map(|row| {
            Ok(Dimension {
                column: read_column(row, 0)?,
                is_time: read_column::<String>(row, 1)? == "Time",
                time_interval: read_column(row, 2)?,
                integer_interval: read_column(row, 3)?,
                partitions: read_column(row, 4)?,
            })
        })
        .collect::<Result<Vec<_>, DriverError>>()?;
    Ok(partitioning_statements(qualified_name, &dimensions))
}

pub(crate) async fn continuous_aggregate_definition(
    client: &Client,
    qualified_name: &str,
    view: &TableRef,
) -> Result<Option<String>, DriverError> {
    if !is_installed(client).await? {
        return Ok(None);
    }
    let row = client
        .query_opt(CONTINUOUS_AGGREGATE_SQL, &[&view.schema, &view.name])
        .await
        .map_err(map_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let query: Option<String> = read_column(&row, 0)?;
    Ok(Some(create_continuous_aggregate(
        qualified_name,
        query.as_deref().unwrap_or_default(),
    )))
}

async fn is_installed(client: &Client) -> Result<bool, DriverError> {
    let row = client
        .query_one(INSTALLED_SQL, &[])
        .await
        .map_err(map_error)?;
    read_column(&row, 0)
}

async fn qualified_names(
    client: &Client,
    sql: &str,
    schema: Option<&str>,
) -> Result<HashSet<QualifiedName>, DriverError> {
    let rows = client.query(sql, &[&schema]).await.map_err(map_error)?;
    rows.iter()
        .map(|row| Ok((read_column(row, 0)?, read_column(row, 1)?)))
        .collect()
}

fn labelled(label: &str, comment: Option<String>) -> String {
    match comment {
        Some(comment) => format!("{label}: {comment}"),
        None => label.to_owned(),
    }
}

fn partitioning_statements(qualified_name: &str, dimensions: &[Dimension]) -> Option<String> {
    let (first, others) = dimensions.split_first()?;
    let relation = Dialect::Postgres.quote_literal(qualified_name);
    let mut statements = vec![format!(
        "SELECT create_hypertable({relation}, {});",
        dimension_builder(first)
    )];
    statements.extend(others.iter().map(|dimension| {
        format!(
            "SELECT add_dimension({relation}, {});",
            dimension_builder(dimension)
        )
    }));
    Some(statements.join("\n"))
}

fn dimension_builder(dimension: &Dimension) -> String {
    let column = Dialect::Postgres.quote_literal(&dimension.column);
    match (dimension.is_time, dimension.partitions) {
        (false, Some(partitions)) => format!("by_hash({column}, {partitions})"),
        _ => match (&dimension.time_interval, dimension.integer_interval) {
            (Some(interval), _) => format!(
                "by_range({column}, INTERVAL {})",
                Dialect::Postgres.quote_literal(interval)
            ),
            (None, Some(interval)) => format!("by_range({column}, {interval})"),
            (None, None) => format!("by_range({column})"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(schema: &str, name: &str, kind: ObjectKind, comment: Option<&str>) -> ObjectInfo {
        ObjectInfo {
            object: TableRef::new(Some("db".into()), Some(schema.into()), name),
            kind,
            comment: comment.map(str::to_owned),
        }
    }

    #[test]
    fn hypertables_and_continuous_aggregates_are_marked() {
        let catalog = TimescaleCatalog {
            hypertables: HashSet::from([("public".to_owned(), "metrics".to_owned())]),
            continuous_aggregates: HashSet::from([("public".to_owned(), "hourly".to_owned())]),
        };
        let mut objects = vec![
            object("public", "metrics", ObjectKind::Table, Some("raw data")),
            object("public", "hourly", ObjectKind::View, None),
            object("public", "plain", ObjectKind::Table, None),
        ];
        catalog.annotate(&mut objects);
        assert_eq!(objects[0].comment.as_deref(), Some("hypertable: raw data"));
        assert_eq!(objects[1].kind, ObjectKind::MaterializedView);
        assert_eq!(objects[1].comment.as_deref(), Some("continuous aggregate"));
        assert_eq!(objects[2].comment, None);
    }

    #[test]
    fn dimensions_become_hypertable_calls() {
        let dimensions = vec![
            Dimension {
                column: "time".into(),
                is_time: true,
                time_interval: Some("7 days".into()),
                integer_interval: None,
                partitions: None,
            },
            Dimension {
                column: "device".into(),
                is_time: false,
                time_interval: None,
                integer_interval: None,
                partitions: Some(4),
            },
        ];
        assert_eq!(
            partitioning_statements("\"public\".\"metrics\"", &dimensions).as_deref(),
            Some(
                "SELECT create_hypertable('\"public\".\"metrics\"', by_range('time', INTERVAL '7 days'));\n\
                 SELECT add_dimension('\"public\".\"metrics\"', by_hash('device', 4));"
            )
        );
        assert_eq!(partitioning_statements("t", &[]), None);
    }
}
