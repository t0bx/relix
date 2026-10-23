/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connector::MsSqlClient;
use crate::errors::map_error;
use relix_core::Dialect;
use relix_driver_api::DriverError;
use tiberius::{Row, ToSql};

const DATABASE_PLACEHOLDER: &str = "{db}";

pub async fn query_rows(
    client: &mut MsSqlClient,
    sql: &str,
    params: &[&dyn ToSql],
) -> Result<Vec<Row>, DriverError> {
    client
        .query(sql, params)
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)
}

pub async fn run_batch(client: &mut MsSqlClient, sql: &str) -> Result<(), DriverError> {
    client
        .simple_query(sql)
        .await
        .map_err(map_error)?
        .into_results()
        .await
        .map_err(map_error)
        .map(drop)
}

/// Catalog views are per database; templates refer to them as `{db}.sys.…`, so one
/// connection can inspect every database without switching its context.
pub fn in_database(template: &str, database: &str) -> String {
    template.replace(
        DATABASE_PLACEHOLDER,
        &Dialect::MsSql.quote_identifier(database),
    )
}

pub fn text_at(row: &Row, index: usize) -> Option<String> {
    row.try_get::<&str, _>(index)
        .ok()
        .flatten()
        .map(str::to_owned)
}

pub fn int_at(row: &Row, index: usize) -> Option<i32> {
    row.try_get::<i32, _>(index).ok().flatten()
}

pub fn flag_at(row: &Row, index: usize) -> bool {
    row.try_get::<bool, _>(index)
        .ok()
        .flatten()
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_names_are_quoted_into_templates() {
        assert_eq!(
            in_database("SELECT name FROM {db}.sys.tables", "sa]les"),
            "SELECT name FROM [sa]]les].sys.tables"
        );
    }
}
