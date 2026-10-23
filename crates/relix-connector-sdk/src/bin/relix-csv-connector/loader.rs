/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::csv_reader::CsvTable;
use relix_core::{Dialect, TableRef};
use relix_driver_api::{DriverError, ExecuteOptions, ParamStyle, SqlBuilder, SqlConnection};

/// Creates one table per CSV file inside a single transaction and then switches the
/// database to `query_only` mode.
pub(crate) async fn load_tables(
    sql: &dyn SqlConnection,
    tables: &[CsvTable],
) -> Result<(), DriverError> {
    run(sql, "BEGIN").await?;
    if let Err(error) = insert_tables(sql, tables).await {
        let _ = run(sql, "ROLLBACK").await;
        return Err(error);
    }
    run(sql, "COMMIT").await?;
    run(sql, "PRAGMA query_only = ON").await
}

async fn insert_tables(sql: &dyn SqlConnection, tables: &[CsvTable]) -> Result<(), DriverError> {
    let builder = SqlBuilder::new(Dialect::Sqlite, ParamStyle::Inline);
    for table in tables {
        run(sql, &create_statement(table)).await?;
        let target = TableRef::new(None, None, &table.name);
        for row in &table.rows {
            let values: Vec<_> = table
                .columns
                .iter()
                .map(|column| column.name.clone())
                .zip(row.iter().cloned())
                .collect();
            run(sql, &builder.insert(&target, &values).sql).await?;
        }
    }
    Ok(())
}

fn create_statement(table: &CsvTable) -> String {
    let columns: Vec<String> = table
        .columns
        .iter()
        .map(|column| {
            format!(
                "{} {}",
                Dialect::Sqlite.quote_identifier(&column.name),
                column.kind.sql_type()
            )
        })
        .collect();
    format!(
        "CREATE TABLE {} ({})",
        Dialect::Sqlite.quote_identifier(&table.name),
        columns.join(", ")
    )
}

async fn run(sql: &dyn SqlConnection, statement: &str) -> Result<(), DriverError> {
    sql.execute(statement, &ExecuteOptions::default()).await?;
    Ok(())
}
