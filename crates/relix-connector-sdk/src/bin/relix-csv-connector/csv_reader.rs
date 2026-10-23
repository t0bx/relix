/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::column_kind::ColumnKind;
use relix_core::Value;
use relix_driver_api::DriverError;
use std::collections::HashSet;
use std::path::Path;

pub(crate) struct CsvColumn {
    pub(crate) name: String,
    pub(crate) kind: ColumnKind,
}

pub(crate) struct CsvTable {
    pub(crate) name: String,
    pub(crate) columns: Vec<CsvColumn>,
    pub(crate) rows: Vec<Vec<Value>>,
}

/// Reads every `*.csv` file directly inside `directory` (sorted by name). The file stem
/// becomes the table name and the header row the column names; files without a header
/// are skipped.
pub(crate) fn read_directory(directory: &Path) -> Result<Vec<CsvTable>, DriverError> {
    if !directory.is_dir() {
        return Err(DriverError::configuration(format!(
            "{} is not a directory of CSV files",
            directory.display()
        )));
    }
    let entries = std::fs::read_dir(directory).map_err(|error| {
        DriverError::connection(format!("cannot read {}: {error}", directory.display()))
    })?;
    let mut files: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
        })
        .collect();
    files.sort();
    let mut tables = Vec::new();
    for file in files {
        if let Some(table) = read_file(&file)? {
            tables.push(table);
        }
    }
    Ok(tables)
}

fn read_file(path: &Path) -> Result<Option<CsvTable>, DriverError> {
    let invalid =
        |error: csv::Error| DriverError::query(format!("cannot read {}: {error}", path.display()));
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(invalid)?;
    let headers = reader.headers().map_err(invalid)?.clone();
    if headers.is_empty() {
        return Ok(None);
    }
    let mut raw_rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(invalid)?;
        let row: Vec<String> = (0..headers.len())
            .map(|index| record.get(index).unwrap_or_default().to_owned())
            .collect();
        raw_rows.push(row);
    }
    let columns: Vec<CsvColumn> = unique_names(headers.iter())
        .into_iter()
        .enumerate()
        .map(|(index, name)| CsvColumn {
            name,
            kind: ColumnKind::infer(raw_rows.iter().map(|row| row[index].as_str())),
        })
        .collect();
    let rows = raw_rows
        .iter()
        .map(|row| {
            row.iter()
                .zip(&columns)
                .map(|(raw, column)| column.kind.value(raw))
                .collect()
        })
        .collect();
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Some(CsvTable {
        name,
        columns,
        rows,
    }))
}

fn unique_names<'a>(headers: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = HashSet::new();
    headers
        .enumerate()
        .map(|(index, header)| {
            let base = match header.trim() {
                "" => format!("column_{}", index + 1),
                trimmed => trimmed.to_owned(),
            };
            let mut candidate = base.clone();
            let mut suffix = 2;
            while !seen.insert(candidate.to_lowercase()) {
                candidate = format!("{base}_{suffix}");
                suffix += 1;
            }
            candidate
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_names_are_made_unique() {
        let names = unique_names(["id", "", "Id", "name"].into_iter());
        assert_eq!(names, ["id", "column_2", "Id_2", "name"]);
    }
}
