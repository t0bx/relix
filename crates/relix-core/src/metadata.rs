/*
 * @author t0bx
 * @since 23.10.2026
 */

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseInfo {
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Table,
    View,
    MaterializedView,
    Function,
    Procedure,
    Sequence,
    Trigger,
}

impl ObjectKind {
    pub fn is_relation(self) -> bool {
        matches!(self, Self::Table | Self::View | Self::MaterializedView)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::View => "view",
            Self::MaterializedView => "materialized view",
            Self::Function => "function",
            Self::Procedure => "procedure",
            Self::Sequence => "sequence",
            Self::Trigger => "trigger",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TableRef {
    pub database: Option<String>,
    pub schema: Option<String>,
    pub name: String,
}

impl TableRef {
    pub fn new(database: Option<String>, schema: Option<String>, name: impl Into<String>) -> Self {
        Self {
            database,
            schema,
            name: name.into(),
        }
    }
}

impl fmt::Display for TableRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(schema) = &self.schema {
            write!(f, "{schema}.")?;
        } else if let Some(database) = &self.database {
            write!(f, "{database}.")?;
        }
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectInfo {
    pub object: TableRef,
    pub kind: ObjectKind,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub ordinal: u32,
    pub is_primary_key: bool,
    pub is_auto_increment: bool,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
    pub is_primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignKeyInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub referenced_table: TableRef,
    pub referenced_columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableDetails {
    pub table: TableRef,
    pub kind: ObjectKind,
    pub columns: Vec<ColumnInfo>,
    pub primary_key: Vec<String>,
    pub indexes: Vec<IndexInfo>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
    pub definition: Option<String>,
}

impl TableDetails {
    /// Columns that reliably identify a single row: the primary key, otherwise the
    /// first unique index whose columns are all declared `NOT NULL`.
    pub fn row_identity(&self) -> Option<Vec<String>> {
        if !self.primary_key.is_empty() {
            return Some(self.primary_key.clone());
        }
        self.indexes
            .iter()
            .filter(|index| index.is_unique && !index.columns.is_empty())
            .find(|index| {
                index.columns.iter().all(|name| {
                    self.columns
                        .iter()
                        .any(|column| &column.name == name && !column.nullable)
                })
            })
            .map(|index| index.columns.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, nullable: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            data_type: "int".into(),
            nullable,
            default_value: None,
            ordinal: 0,
            is_primary_key: false,
            is_auto_increment: false,
            comment: None,
        }
    }

    fn details(primary_key: Vec<String>, indexes: Vec<IndexInfo>) -> TableDetails {
        TableDetails {
            table: TableRef::new(None, Some("public".into()), "t"),
            kind: ObjectKind::Table,
            columns: vec![column("a", false), column("b", true)],
            primary_key,
            indexes,
            foreign_keys: vec![],
            definition: None,
        }
    }

    #[test]
    fn primary_key_is_preferred_row_identity() {
        let table = details(vec!["a".into()], vec![]);
        assert_eq!(table.row_identity(), Some(vec!["a".to_owned()]));
    }

    #[test]
    fn unique_not_null_index_is_fallback_identity() {
        let nullable = IndexInfo {
            name: "b_key".into(),
            columns: vec!["b".into()],
            is_unique: true,
            is_primary: false,
        };
        let strict = IndexInfo {
            name: "a_key".into(),
            columns: vec!["a".into()],
            is_unique: true,
            is_primary: false,
        };
        assert_eq!(details(vec![], vec![nullable.clone()]).row_identity(), None);
        assert_eq!(
            details(vec![], vec![nullable, strict]).row_identity(),
            Some(vec!["a".to_owned()])
        );
    }

    #[test]
    fn table_ref_display_prefers_schema() {
        let table = TableRef::new(Some("db".into()), Some("public".into()), "users");
        assert_eq!(table.to_string(), "public.users");
        let mysql = TableRef::new(Some("shop".into()), None, "orders");
        assert_eq!(mysql.to_string(), "shop.orders");
    }
}
