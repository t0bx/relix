/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog_rows::{int_at, query_rows, text_at};
use crate::connector::MsSqlClient;
use relix_driver_api::{DriverError, ServerInfo};

const SERVER_DETAILS_SQL: &str = "SELECT \
     CAST(SERVERPROPERTY('ProductVersion') AS nvarchar(128)), \
     CAST(SERVERPROPERTY('Edition') AS nvarchar(128)), \
     CAST(SERVERPROPERTY('EngineEdition') AS int), \
     CAST(@@SPID AS int), DB_NAME(), SCHEMA_NAME()";

const PROCESS_ID_SQL: &str = "SELECT CAST(@@SPID AS int)";

const AZURE_SQL_DATABASE: i32 = 5;
const AZURE_SQL_MANAGED_INSTANCE: i32 = 8;
const AZURE_SQL_EDGE: i32 = 9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerDetails {
    pub info: ServerInfo,
    pub process_id: i32,
    pub database: String,
    pub schema: String,
}

pub async fn server_details(client: &mut MsSqlClient) -> Result<ServerDetails, DriverError> {
    let rows = query_rows(client, SERVER_DETAILS_SQL, &[]).await?;
    let row = rows
        .first()
        .ok_or_else(|| DriverError::internal("server details query returned no row"))?;
    let version = text_at(row, 0).unwrap_or_default();
    let edition = text_at(row, 1).unwrap_or_default();
    Ok(ServerDetails {
        info: ServerInfo {
            product: product_name(int_at(row, 2).unwrap_or_default()).to_owned(),
            version: if edition.is_empty() {
                version
            } else {
                format!("{version} ({edition})")
            },
        },
        process_id: int_at(row, 3).unwrap_or_default(),
        database: text_at(row, 4).unwrap_or_default(),
        schema: text_at(row, 5).unwrap_or_else(|| "dbo".to_owned()),
    })
}

pub async fn process_id(client: &mut MsSqlClient) -> Result<i32, DriverError> {
    let rows = query_rows(client, PROCESS_ID_SQL, &[]).await?;
    rows.first()
        .and_then(|row| int_at(row, 0))
        .ok_or_else(|| DriverError::internal("@@SPID returned no value"))
}

fn product_name(engine_edition: i32) -> &'static str {
    match engine_edition {
        AZURE_SQL_DATABASE => "Azure SQL Database",
        AZURE_SQL_MANAGED_INSTANCE => "Azure SQL Managed Instance",
        AZURE_SQL_EDGE => "Azure SQL Edge",
        _ => "Microsoft SQL Server",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_editions_name_the_product() {
        assert_eq!(product_name(3), "Microsoft SQL Server");
        assert_eq!(product_name(5), "Azure SQL Database");
        assert_eq!(product_name(9), "Azure SQL Edge");
    }
}
