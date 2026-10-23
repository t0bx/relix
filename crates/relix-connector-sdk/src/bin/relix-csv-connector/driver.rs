/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::CsvConnection;
use crate::csv_reader::read_directory;
use crate::loader::load_tables;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, ConnectionSettings, Dialect, DriverId};
use relix_driver_api::{
    ConnectionTarget, DRIVER_API_VERSION, DatabaseConnection, DatabaseDriver, DriverCapabilities,
    DriverError, DriverMetadata, DriverOrigin, with_timeout,
};
use relix_driver_sqlite::SqliteDriver;
use std::path::PathBuf;

pub(crate) struct CsvDriver {
    sqlite: SqliteDriver,
}

impl CsvDriver {
    pub(crate) fn new() -> Self {
        Self {
            sqlite: SqliteDriver::new(),
        }
    }

    async fn open(&self, directory: PathBuf) -> Result<CsvConnection, DriverError> {
        let tables = tokio::task::spawn_blocking(move || read_directory(&directory))
            .await
            .map_err(|error| DriverError::internal(error.to_string()))??;
        let memory = ConnectionSettings {
            file_path: Some(PathBuf::from(":memory:")),
            ..ConnectionSettings::default()
        };
        let connection = CsvConnection::new(
            self.sqlite
                .connect(&ConnectionConfig::new(memory, None))
                .await?,
        );
        load_tables(connection.sql()?, &tables).await?;
        Ok(connection)
    }
}

#[async_trait]
impl DatabaseDriver for CsvDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata {
            id: DriverId::new("csv"),
            display_name: "CSV Files".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            api_version: DRIVER_API_VERSION,
            origin: DriverOrigin::External {
                package: "relix-csv-connector".to_owned(),
            },
            dialect: Some(Dialect::Sqlite),
            target: ConnectionTarget::File,
        }
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            transactions: false,
            cancellation: true,
            table_editing: false,
            ..DriverCapabilities::relational()
        }
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let settings = &config.settings;
        let directory = settings
            .file_path
            .clone()
            .filter(|path| !path.as_os_str().is_empty())
            .or_else(|| {
                settings
                    .database
                    .as_deref()
                    .filter(|name| !name.is_empty())
                    .map(PathBuf::from)
            })
            .ok_or_else(|| {
                DriverError::configuration("choose the directory that contains the CSV files")
            })?;
        let connection = with_timeout(settings.connect_timeout(), self.open(directory)).await?;
        Ok(Box::new(connection))
    }
}
