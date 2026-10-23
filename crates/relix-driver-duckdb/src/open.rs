/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::{map_error, map_open_error};
use duckdb::{AccessMode, Config, Connection};
use relix_core::ConnectionSettings;
use relix_driver_api::DriverError;
use std::path::{Path, PathBuf};

const IN_MEMORY_PATH: &str = ":memory:";
const ACCESS_MODE_PROPERTY: &str = "access_mode";

pub(crate) struct OpenedDatabase {
    pub(crate) session: Connection,
    pub(crate) data: Connection,
    pub(crate) version: String,
}

enum DatabaseTarget {
    Memory,
    File(PathBuf),
}

/// Opens the database once and derives a second connection from the same instance:
/// the session connection serves the editor (and keeps its transactions), the data
/// connection serves metadata, table pages and grid edits so browsing never waits
/// for a long editor query. Both see the same in-memory database and attachments.
pub(crate) fn open_database(settings: &ConnectionSettings) -> Result<OpenedDatabase, DriverError> {
    let config = configuration(settings)?;
    let session = match resolve_target(settings)? {
        DatabaseTarget::Memory => Connection::open_in_memory_with_flags(config),
        DatabaseTarget::File(path) => Connection::open_with_flags(path, config),
    }
    .map_err(map_open_error)?;
    let data = session.try_clone().map_err(map_open_error)?;
    let version = data
        .query_row("SELECT version()", [], |row| row.get::<_, String>(0))
        .map_err(map_error)?;
    Ok(OpenedDatabase {
        session,
        data,
        version,
    })
}

fn resolve_target(settings: &ConnectionSettings) -> Result<DatabaseTarget, DriverError> {
    let path = settings
        .file_path
        .as_deref()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| DriverError::configuration("a database file path is required"))?;
    if path == Path::new(IN_MEMORY_PATH) {
        return Ok(DatabaseTarget::Memory);
    }
    if path.is_dir() {
        return Err(DriverError::configuration(format!(
            "{} is a directory, not a database file",
            path.display()
        )));
    }
    if !path.exists() && !settings.create_if_missing {
        return Err(DriverError::configuration(format!(
            "database file {} does not exist (enable \"create if missing\" to create it)",
            path.display()
        )));
    }
    Ok(DatabaseTarget::File(path.to_path_buf()))
}

fn configuration(settings: &ConnectionSettings) -> Result<Config, DriverError> {
    let mode = match settings.property(ACCESS_MODE_PROPERTY) {
        None => AccessMode::Automatic,
        Some(value) => parse_access_mode(value)?,
    };
    Config::default().access_mode(mode).map_err(map_open_error)
}

fn parse_access_mode(value: &str) -> Result<AccessMode, DriverError> {
    match value.to_ascii_lowercase().replace('-', "_").as_str() {
        "automatic" => Ok(AccessMode::Automatic),
        "read_only" | "readonly" => Ok(AccessMode::ReadOnly),
        "read_write" | "readwrite" => Ok(AccessMode::ReadWrite),
        _ => Err(DriverError::configuration(format!(
            "unknown access_mode \"{value}\" (expected automatic, read_only or read_write)"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_driver_api::ErrorKind;

    fn settings(path: &str) -> ConnectionSettings {
        ConnectionSettings {
            file_path: Some(PathBuf::from(path)),
            ..ConnectionSettings::default()
        }
    }

    #[test]
    fn missing_path_is_a_configuration_error() {
        let error = open_database(&ConnectionSettings::default()).err().unwrap();
        assert_eq!(error.kind, ErrorKind::Configuration);
    }

    #[test]
    fn missing_file_requires_create_if_missing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("absent.duckdb");
        let mut settings = settings(&path.to_string_lossy());
        let error = open_database(&settings).err().unwrap();
        assert_eq!(error.kind, ErrorKind::Configuration);
        settings.create_if_missing = true;
        assert!(open_database(&settings).is_ok());
        assert!(path.exists());
    }

    #[test]
    fn memory_databases_are_shared_between_both_connections() {
        let opened = open_database(&settings(IN_MEMORY_PATH)).unwrap();
        opened
            .session
            .execute_batch("CREATE TABLE shared (a INTEGER)")
            .unwrap();
        let count: i64 = opened
            .data
            .query_row("SELECT count(*) FROM shared", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert!(opened.version.starts_with('v'));
    }

    #[test]
    fn unknown_access_modes_are_rejected() {
        let mut settings = settings(IN_MEMORY_PATH);
        settings
            .properties
            .insert(ACCESS_MODE_PROPERTY.into(), "sometimes".into());
        let error = open_database(&settings).err().unwrap();
        assert_eq!(error.kind, ErrorKind::Configuration);
        assert!(matches!(
            parse_access_mode("Read-Only"),
            Ok(AccessMode::ReadOnly)
        ));
    }
}
