/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{Dialect, ObjectKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogColumn {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogTable {
    pub schema: Option<String>,
    pub name: String,
    pub kind: ObjectKind,
    pub columns: Option<Vec<CatalogColumn>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionCatalog {
    pub dialect: Dialect,
    pub default_schema: Option<String>,
    pub schemas: Vec<String>,
    pub tables: Vec<CatalogTable>,
    pub routines: Vec<String>,
    pub fragments: Vec<String>,
}

impl CompletionCatalog {
    pub fn empty(dialect: Dialect) -> Self {
        Self {
            dialect,
            default_schema: None,
            schemas: Vec::new(),
            tables: Vec::new(),
            routines: Vec::new(),
            fragments: Vec::new(),
        }
    }

    /// Resolves a table name case-insensitively. Without an explicit schema, a table in the
    /// default schema wins over equally named tables elsewhere.
    pub fn find_table(&self, schema: Option<&str>, name: &str) -> Option<&CatalogTable> {
        let matches_name = |table: &&CatalogTable| table.name.eq_ignore_ascii_case(name);
        match schema {
            Some(schema) => self.tables.iter().filter(matches_name).find(|table| {
                table
                    .schema
                    .as_deref()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(schema))
            }),
            None => {
                let mut candidates = self.tables.iter().filter(matches_name);
                let first = candidates.next()?;
                if self.is_default_schema(first.schema.as_deref()) {
                    return Some(first);
                }
                candidates
                    .find(|table| self.is_default_schema(table.schema.as_deref()))
                    .or(Some(first))
            }
        }
    }

    pub fn is_default_schema(&self, schema: Option<&str>) -> bool {
        match (schema, self.default_schema.as_deref()) {
            (None, _) => true,
            (Some(schema), Some(default)) => schema.eq_ignore_ascii_case(default),
            (Some(_), None) => false,
        }
    }

    pub fn has_schema(&self, name: &str) -> bool {
        self.schemas
            .iter()
            .any(|schema| schema.eq_ignore_ascii_case(name))
    }

    pub fn set_columns(&mut self, schema: Option<&str>, name: &str, columns: Vec<CatalogColumn>) {
        let target = self
            .tables
            .iter_mut()
            .find(|table| table.name == name && table.schema.as_deref() == schema);
        if let Some(table) = target {
            table.columns = Some(columns);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(schema: &str, name: &str) -> CatalogTable {
        CatalogTable {
            schema: Some(schema.into()),
            name: name.into(),
            kind: ObjectKind::Table,
            columns: None,
        }
    }

    #[test]
    fn prefers_default_schema() {
        let mut catalog = CompletionCatalog::empty(Dialect::Postgres);
        catalog.default_schema = Some("public".into());
        catalog.tables = vec![table("audit", "users"), table("public", "users")];
        assert_eq!(
            catalog.find_table(None, "USERS").unwrap().schema.as_deref(),
            Some("public")
        );
        assert_eq!(
            catalog
                .find_table(Some("audit"), "users")
                .unwrap()
                .schema
                .as_deref(),
            Some("audit")
        );
        catalog.set_columns(
            Some("public"),
            "users",
            vec![CatalogColumn {
                name: "id".into(),
                data_type: "int".into(),
            }],
        );
        assert!(catalog.find_table(None, "users").unwrap().columns.is_some());
    }
}
