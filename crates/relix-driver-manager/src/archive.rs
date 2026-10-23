/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ManagerError;
use flate2::read::GzDecoder;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use tar::{Archive, EntryType};

pub const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;

/// Unpacks a `.tar.gz` package below `destination`. Only plain files and directories with
/// relative, non-escaping paths are accepted; links, devices and absolute or `..` paths
/// reject the whole package. File modes from the archive are ignored and the total
/// unpacked size is capped to defuse decompression bombs.
pub fn extract_package(bytes: &[u8], destination: &Path) -> Result<(), ManagerError> {
    let mut archive = Archive::new(GzDecoder::new(bytes));
    let entries = archive.entries().map_err(invalid_archive)?;
    let mut remaining = MAX_UNPACKED_BYTES;
    for (count, entry) in entries.enumerate() {
        if count >= MAX_ENTRIES {
            return Err(ManagerError::InvalidPackage(format!(
                "archive contains more than {MAX_ENTRIES} entries"
            )));
        }
        let mut entry = entry.map_err(invalid_archive)?;
        let relative = safe_relative_path(&entry.path().map_err(invalid_archive)?)?;
        let Some(relative) = relative else {
            continue;
        };
        let target = destination.join(&relative);
        match entry.header().entry_type() {
            EntryType::Directory => {
                fs::create_dir_all(&target).map_err(|error| ManagerError::io(&target, error))?;
            }
            EntryType::Regular | EntryType::Continuous => {
                remaining = write_file(&mut entry, &target, remaining)?;
            }
            other => {
                return Err(ManagerError::InvalidPackage(format!(
                    "{} has unsupported entry type {other:?}",
                    relative.display()
                )));
            }
        }
    }
    Ok(())
}

fn write_file(entry: &mut impl Read, target: &Path, remaining: u64) -> Result<u64, ManagerError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| ManagerError::io(parent, error))?;
    }
    let mut file = fs::File::create(target).map_err(|error| ManagerError::io(target, error))?;
    let written = io::copy(&mut entry.take(remaining + 1), &mut file)
        .map_err(|error| ManagerError::io(target, error))?;
    if written > remaining {
        return Err(ManagerError::InvalidPackage(format!(
            "unpacked contents exceed {MAX_UNPACKED_BYTES} bytes"
        )));
    }
    Ok(remaining - written)
}

pub(crate) fn safe_relative_path(path: &Path) -> Result<Option<PathBuf>, ManagerError> {
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ManagerError::InvalidPackage(format!(
                    "path {} escapes the package directory",
                    path.display()
                )));
            }
        }
    }
    Ok((!relative.as_os_str().is_empty()).then_some(relative))
}

fn invalid_archive(error: io::Error) -> ManagerError {
    ManagerError::InvalidPackage(format!("unreadable archive: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;

    fn archive_with(path: &str, entry_type: EntryType, contents: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(entry_type);
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        {
            let name = header.as_old_mut().name.as_mut_slice();
            name[..path.len()].copy_from_slice(path.as_bytes());
        }
        header.set_cksum();
        builder.append(&header, contents).unwrap();
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn extracts_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        extract_package(
            &archive_with("./bin/tool", EntryType::Regular, b"x"),
            dir.path(),
        )
        .unwrap();
        assert_eq!(fs::read(dir.path().join("bin/tool")).unwrap(), b"x");
    }

    #[test]
    fn rejects_escaping_paths_and_links() {
        let dir = tempfile::tempdir().unwrap();
        for (path, kind) in [
            ("../evil", EntryType::Regular),
            ("/etc/evil", EntryType::Regular),
            ("link", EntryType::Symlink),
        ] {
            let result = extract_package(&archive_with(path, kind, b""), dir.path());
            assert!(
                matches!(result, Err(ManagerError::InvalidPackage(_))),
                "{path}"
            );
        }
        assert!(!dir.path().parent().unwrap().join("evil").exists());
    }
}
