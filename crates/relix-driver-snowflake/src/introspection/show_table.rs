/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::result::RawRows;

/// Result of a `SHOW`/`DESC` command with cells addressed by (case-insensitive) column
/// name, since Snowflake documents these outputs by name rather than position.
pub(crate) struct ShowTable {
    columns: Vec<String>,
    rows: Vec<Vec<Option<String>>>,
}

impl ShowTable {
    pub(crate) fn rows(&self) -> impl Iterator<Item = ShowRow<'_>> {
        self.rows.iter().map(|values| ShowRow {
            columns: &self.columns,
            values,
        })
    }
}

impl From<RawRows> for ShowTable {
    fn from(raw: RawRows) -> Self {
        Self {
            columns: raw
                .columns
                .iter()
                .map(|column| column.name.to_lowercase())
                .collect(),
            rows: raw.rows,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ShowRow<'a> {
    columns: &'a [String],
    values: &'a [Option<String>],
}

impl<'a> ShowRow<'a> {
    pub(crate) fn get(&self, column: &str) -> Option<&'a str> {
        let index = self.columns.iter().position(|name| name == column)?;
        self.values.get(index)?.as_deref()
    }

    pub(crate) fn flag(&self, column: &str) -> bool {
        self.get(column).is_some_and(|value| {
            value.eq_ignore_ascii_case("Y") || value.eq_ignore_ascii_case("true")
        })
    }

    pub(crate) fn non_empty(&self, column: &str) -> Option<String> {
        self.get(column)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }
}
