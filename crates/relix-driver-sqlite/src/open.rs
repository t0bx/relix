/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use relix_core::ConnectionSettings;
use relix_driver_api::DriverError;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::Duration;

const IN_MEMORY_PATH: &str = ":memory:";
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

enum DatabaseTarget {
    Memory,
    File { path: PathBuf, create: bool },
}

pub(crate) fn open_database(settings: &ConnectionSettings) -> Result<Connection, DriverError> {
    let connection = match resolve_target(settings)? {
        DatabaseTarget::Memory => Connection::open_in_memory().map_err(map_error)?,
        DatabaseTarget::File { path, create } => open_file(&path, create)?,
    };
    configure(&connection)?;
    Ok(connection)
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
    Ok(DatabaseTarget::File {
        path: path.to_path_buf(),
        create: settings.create_if_missing,
    })
}

fn open_file(path: &Path, create: bool) -> Result<Connection, DriverError> {
    if path.is_dir() {
        return Err(DriverError::configuration(format!(
            "{} is a directory, not a database file",
            path.display()
        )));
    }
    let mut flags = OpenFlags::default();
    if !path.exists() {
        if !create {
            return Err(DriverError::configuration(format!(
                "database file {} does not exist (enable \"create if missing\" to create it)",
                path.display()
            )));
        }
    } else {
        flags.remove(OpenFlags::SQLITE_OPEN_CREATE);
    }
    flags.remove(OpenFlags::SQLITE_OPEN_URI);
    Connection::open_with_flags(path, flags).map_err(map_error)
}

fn configure(connection: &Connection) -> Result<(), DriverError> {
    connection.busy_timeout(BUSY_TIMEOUT).map_err(map_error)?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(map_error)?;
    connection
        .query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
        .map_err(map_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_driver_api::ErrorKind;

    #[test]
    fn missing_path_is_a_configuration_error() {
        let error = open_database(&ConnectionSettings::default()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Configuration);
    }

    #[test]
    fn memory_databases_enable_foreign_keys() {
        let settings = ConnectionSettings {
            file_path: Some(PathBuf::from(IN_MEMORY_PATH)),
            ..ConnectionSettings::default()
        };
        let connection = open_database(&settings).unwrap();
        let enabled: bool = connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert!(enabled);
    }
}
