/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ProjectError;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

pub fn append<T: Serialize>(path: &Path, entry: &T) -> Result<(), ProjectError> {
    let line = serde_json::to_string(entry).map_err(|error| ProjectError::Corrupt {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| ProjectError::io(path, error))?;
    writeln!(file, "{line}").map_err(|error| ProjectError::io(path, error))
}

/// Reads the newest `limit` entries. Lines that fail to parse (for example a line cut
/// short by a crash) are skipped rather than invalidating the whole history.
pub fn read_tail<T: DeserializeOwned>(path: &Path, limit: usize) -> Result<Vec<T>, ProjectError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(ProjectError::io(path, error)),
    };
    let mut newest = VecDeque::with_capacity(limit.min(4096));
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| ProjectError::io(path, error))?;
        let Ok(entry) = serde_json::from_str::<T>(&line) else {
            continue;
        };
        if newest.len() == limit {
            newest.pop_front();
        }
        if limit > 0 {
            newest.push_back(entry);
        }
    }
    Ok(newest.into())
}

pub fn rewrite<T: Serialize>(path: &Path, entries: &[T]) -> Result<(), ProjectError> {
    let mut text = String::new();
    for entry in entries {
        let line = serde_json::to_string(entry).map_err(|error| ProjectError::Corrupt {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        text.push_str(&line);
        text.push('\n');
    }
    relix_config::files::write_atomic(path, text.as_bytes()).map_err(ProjectError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_newest_entries_and_skips_broken_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        for value in 0..5 {
            append(&path, &value).unwrap();
        }
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{broken\n")
            .unwrap();
        assert_eq!(read_tail::<i32>(&path, 3).unwrap(), vec![2, 3, 4]);
        rewrite(&path, &[9]).unwrap();
        assert_eq!(read_tail::<i32>(&path, 10).unwrap(), vec![9]);
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            read_tail::<i32>(&dir.path().join("none"), 5)
                .unwrap()
                .is_empty()
        );
    }
}
