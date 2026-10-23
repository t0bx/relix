/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ConfigError;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn ensure_private_dir(path: &Path) -> Result<(), ConfigError> {
    fs::create_dir_all(path).map_err(|error| ConfigError::io(path, error))?;
    restrict_permissions(path, 0o700)
}

/// Writes through a temporary sibling file and renames it into place, so a crash never
/// leaves a half-written configuration behind. Files are readable by the owner only.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let temporary = temporary_sibling(path);
    let mut file =
        fs::File::create(&temporary).map_err(|error| ConfigError::io(&temporary, error))?;
    restrict_permissions(&temporary, 0o600)?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|error| ConfigError::io(&temporary, error))?;
    drop(file);
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        ConfigError::io(path, error)
    })
}

pub fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, ConfigError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ConfigError::io(path, error)),
    };
    toml::from_str(&text)
        .map(Some)
        .map_err(|error| ConfigError::Parse {
            path: path.to_path_buf(),
            message: error.message().to_owned(),
        })
}

pub fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<(), ConfigError> {
    let text =
        toml::to_string_pretty(value).map_err(|error| ConfigError::Serialize(error.to_string()))?;
    write_atomic(path, text.as_bytes())
}

/// Moves an unreadable file aside (`name.corrupt-N`) so defaults can be written without
/// destroying what the user had. Returns the backup location.
pub fn quarantine(path: &Path) -> Result<PathBuf, ConfigError> {
    let mut attempt = 0u32;
    loop {
        let candidate = path.with_extension(format!("corrupt-{attempt}"));
        if !candidate.exists() {
            fs::rename(path, &candidate).map_err(|error| ConfigError::io(path, error))?;
            return Ok(candidate);
        }
        attempt += 1;
    }
}

fn temporary_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".to_owned());
    path.with_file_name(format!(".{name}.tmp-{}", std::process::id()))
}

#[cfg(unix)]
fn restrict_permissions(path: &Path, mode: u32) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| ConfigError::io(path, error))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path, _mode: u32) -> Result<(), ConfigError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        name: String,
    }

    #[test]
    fn toml_roundtrip_and_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("sample.toml");
        assert_eq!(read_toml::<Sample>(&path).unwrap(), None);
        write_toml(&path, &Sample { name: "x".into() }).unwrap();
        assert_eq!(
            read_toml::<Sample>(&path).unwrap(),
            Some(Sample { name: "x".into() })
        );
    }

    #[cfg(unix)]
    #[test]
    fn written_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.toml");
        write_atomic(&path, b"a = 1").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn quarantine_moves_file_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "garbage = [").unwrap();
        let backup = quarantine(&path).unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read_to_string(backup).unwrap(), "garbage = [");
    }
}
