/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::cql_rows::select_rows;
use relix_driver_api::DriverError;
use scylla::client::session::Session;

const COLUMNS_CQL: &str = "SELECT column_name, kind, position, type, clustering_order \
     FROM system_schema.columns WHERE keyspace_name = ? AND table_name = ?";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ColumnKind {
    PartitionKey { position: i32 },
    Clustering { position: i32, descending: bool },
    Static,
    Regular,
}

impl ColumnKind {
    fn parse(kind: &str, position: i32, clustering_order: &str) -> Self {
        match kind {
            "partition_key" => Self::PartitionKey { position },
            "clustering" => Self::Clustering {
                position,
                descending: clustering_order.eq_ignore_ascii_case("desc"),
            },
            "static" => Self::Static,
            _ => Self::Regular,
        }
    }

    pub(crate) fn is_primary_key(&self) -> bool {
        matches!(self, Self::PartitionKey { .. } | Self::Clustering { .. })
    }

    pub(crate) fn describe(&self) -> String {
        match self {
            Self::PartitionKey { position } => format!("partition key #{}", position + 1),
            Self::Clustering {
                position,
                descending,
            } => format!(
                "clustering column #{} ({})",
                position + 1,
                if *descending { "DESC" } else { "ASC" }
            ),
            Self::Static => "static column".to_owned(),
            Self::Regular => "regular column".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SchemaColumn {
    pub(crate) name: String,
    pub(crate) kind: ColumnKind,
    pub(crate) data_type: String,
}

/// Loads the columns of a table or materialized view in the order `DESCRIBE` uses:
/// partition key, clustering columns, static columns and regular columns, the latter
/// two alphabetically.
pub(crate) async fn load_columns(
    session: &Session,
    keyspace: &str,
    table: &str,
) -> Result<Vec<SchemaColumn>, DriverError> {
    let rows: Vec<(String, String, i32, String, String)> =
        select_rows(session, COLUMNS_CQL, (keyspace, table)).await?;
    let mut columns: Vec<SchemaColumn> = rows
        .into_iter()
        .map(|(name, kind, position, data_type, order)| SchemaColumn {
            kind: ColumnKind::parse(&kind, position, &order),
            name,
            data_type,
        })
        .collect();
    sort_columns(&mut columns);
    Ok(columns)
}

fn sort_columns(columns: &mut [SchemaColumn]) {
    columns.sort_by(|left, right| {
        let rank = |column: &SchemaColumn| match column.kind {
            ColumnKind::PartitionKey { position } => (0, position),
            ColumnKind::Clustering { position, .. } => (1, position),
            ColumnKind::Static => (2, 0),
            ColumnKind::Regular => (3, 0),
        };
        rank(left)
            .cmp(&rank(right))
            .then_with(|| left.name.cmp(&right.name))
    });
}

pub(crate) fn primary_key(columns: &[SchemaColumn]) -> Vec<String> {
    columns
        .iter()
        .filter(|column| column.kind.is_primary_key())
        .map(|column| column.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, kind: ColumnKind) -> SchemaColumn {
        SchemaColumn {
            name: name.into(),
            kind,
            data_type: "text".into(),
        }
    }

    #[test]
    fn columns_follow_primary_key_order() {
        let mut columns = vec![
            column("zeta", ColumnKind::Regular),
            column("alpha", ColumnKind::Regular),
            column("shared", ColumnKind::Static),
            column(
                "ck",
                ColumnKind::Clustering {
                    position: 0,
                    descending: true,
                },
            ),
            column("pk2", ColumnKind::PartitionKey { position: 1 }),
            column("pk1", ColumnKind::PartitionKey { position: 0 }),
        ];
        sort_columns(&mut columns);
        let names: Vec<&str> = columns.iter().map(|column| column.name.as_str()).collect();
        assert_eq!(names, ["pk1", "pk2", "ck", "shared", "alpha", "zeta"]);
        assert_eq!(primary_key(&columns), ["pk1", "pk2", "ck"]);
    }

    #[test]
    fn kinds_are_parsed_and_described() {
        assert_eq!(
            ColumnKind::parse("clustering", 1, "DESC").describe(),
            "clustering column #2 (DESC)"
        );
        assert_eq!(
            ColumnKind::parse("partition_key", 0, "none").describe(),
            "partition key #1"
        );
        assert!(!ColumnKind::parse("static", -1, "none").is_primary_key());
    }
}
