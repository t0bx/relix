/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::table_structure::{
    ColumnDefinition, ForeignKeyDefinition, IndexDefinition, TableStructure,
};
use relix_core::{Dialect, TableRef};

const NO_ACTION: &str = "NO_ACTION";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceDefinition {
    pub type_name: String,
    pub start: String,
    pub increment: String,
    pub minimum: String,
    pub maximum: String,
    pub cycling: bool,
    pub cache: Option<Option<i32>>,
}

/// SQL Server has no `SHOW CREATE TABLE`; the statement is generated from the catalog
/// and covers columns (types, identity, computed expressions, defaults, nullability),
/// primary key, unique constraints, foreign keys and further indexes.
pub fn create_table(table: &TableRef, structure: &TableStructure) -> String {
    let name = schema_qualified(table);
    let mut lines: Vec<String> = structure.columns.iter().map(column_line).collect();
    lines.extend(
        structure
            .indexes
            .iter()
            .filter(|index| index.is_primary || index.is_unique_constraint)
            .map(constraint_line),
    );
    lines.extend(structure.foreign_keys.iter().map(foreign_key_line));
    let mut ddl = format!("CREATE TABLE {name} (\n    {}\n);", lines.join(",\n    "));
    for index in structure
        .indexes
        .iter()
        .filter(|index| !index.is_primary && !index.is_unique_constraint)
    {
        ddl.push_str(&format!(
            "\nCREATE {}{} INDEX {} ON {name} ({});",
            if index.is_unique { "UNIQUE " } else { "" },
            clustering(index),
            quote(&index.name),
            index_columns(index)
        ));
    }
    ddl
}

pub fn create_sequence(sequence: &TableRef, definition: &SequenceDefinition) -> String {
    let cache = match definition.cache {
        None => "NO CACHE".to_owned(),
        Some(None) => "CACHE".to_owned(),
        Some(Some(size)) => format!("CACHE {size}"),
    };
    format!(
        "CREATE SEQUENCE {} AS {} START WITH {} INCREMENT BY {} MINVALUE {} MAXVALUE {} {} {};",
        schema_qualified(sequence),
        definition.type_name,
        definition.start,
        definition.increment,
        definition.minimum,
        definition.maximum,
        if definition.cycling {
            "CYCLE"
        } else {
            "NO CYCLE"
        },
        cache
    )
}

fn column_line(column: &ColumnDefinition) -> String {
    let name = quote(&column.info.name);
    if let Some(expression) = &column.computed {
        return format!("{name} AS {expression}");
    }
    let mut line = format!("{name} {}", column.info.data_type);
    if let Some((seed, increment)) = &column.identity {
        line.push_str(&format!(" IDENTITY({seed},{increment})"));
    }
    line.push_str(if column.info.nullable {
        " NULL"
    } else {
        " NOT NULL"
    });
    if let Some(default) = &column.info.default_value {
        line.push_str(&format!(" DEFAULT {default}"));
    }
    line
}

fn constraint_line(index: &IndexDefinition) -> String {
    format!(
        "CONSTRAINT {} {} {} ({})",
        quote(&index.name),
        if index.is_primary {
            "PRIMARY KEY"
        } else {
            "UNIQUE"
        },
        clustering(index),
        index_columns(index)
    )
}

fn foreign_key_line(key: &ForeignKeyDefinition) -> String {
    let mut line = format!(
        "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
        quote(&key.info.name),
        quoted_list(&key.info.columns),
        schema_qualified(&key.info.referenced_table),
        quoted_list(&key.info.referenced_columns)
    );
    for (event, action) in [("DELETE", &key.on_delete), ("UPDATE", &key.on_update)] {
        if !action.is_empty() && action != NO_ACTION {
            line.push_str(&format!(" ON {event} {}", action.replace('_', " ")));
        }
    }
    line
}

fn clustering(index: &IndexDefinition) -> &'static str {
    if index.is_clustered {
        "CLUSTERED"
    } else {
        "NONCLUSTERED"
    }
}

fn index_columns(index: &IndexDefinition) -> String {
    index
        .columns
        .iter()
        .map(|(column, descending)| {
            format!(
                "{} {}",
                quote(column),
                if *descending { "DESC" } else { "ASC" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn quoted_list(columns: &[String]) -> String {
    columns
        .iter()
        .map(|column| quote(column))
        .collect::<Vec<_>>()
        .join(", ")
}

fn schema_qualified(object: &TableRef) -> String {
    match &object.schema {
        Some(schema) => format!("{}.{}", quote(schema), quote(&object.name)),
        None => quote(&object.name),
    }
}

fn quote(identifier: &str) -> String {
    Dialect::MsSql.quote_identifier(identifier)
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::{ColumnInfo, ForeignKeyInfo};

    fn column(name: &str, data_type: &str, nullable: bool) -> ColumnDefinition {
        ColumnDefinition {
            info: ColumnInfo {
                name: name.into(),
                data_type: data_type.into(),
                nullable,
                default_value: None,
                ordinal: 0,
                is_primary_key: false,
                is_auto_increment: false,
                comment: None,
            },
            computed: None,
            identity: None,
        }
    }

    #[test]
    fn tables_render_columns_constraints_and_indexes() {
        let mut id = column("id", "int", false);
        id.identity = Some(("1".into(), "1".into()));
        let mut code = column("code", "nvarchar(20)", false);
        code.info.default_value = Some("(N'x')".into());
        let mut total = column("total", "int", true);
        total.computed = Some("([id]*(2))".into());
        let structure = TableStructure {
            columns: vec![id, code, total],
            indexes: vec![
                IndexDefinition {
                    name: "PK_items".into(),
                    is_unique: true,
                    is_primary: true,
                    is_unique_constraint: false,
                    is_clustered: true,
                    columns: vec![("id".into(), false)],
                },
                IndexDefinition {
                    name: "ix_code".into(),
                    is_unique: false,
                    is_primary: false,
                    is_unique_constraint: false,
                    is_clustered: false,
                    columns: vec![("code".into(), true)],
                },
            ],
            foreign_keys: vec![ForeignKeyDefinition {
                info: ForeignKeyInfo {
                    name: "fk_parent".into(),
                    columns: vec!["code".into()],
                    referenced_table: TableRef::new(None, Some("dbo".into()), "parents"),
                    referenced_columns: vec!["code".into()],
                },
                on_delete: "CASCADE".into(),
                on_update: "NO_ACTION".into(),
            }],
        };
        let table = TableRef::new(Some("shop".into()), Some("dbo".into()), "items");
        assert_eq!(
            create_table(&table, &structure),
            "CREATE TABLE [dbo].[items] (\n    \
             [id] int IDENTITY(1,1) NOT NULL,\n    \
             [code] nvarchar(20) NOT NULL DEFAULT (N'x'),\n    \
             [total] AS ([id]*(2)),\n    \
             CONSTRAINT [PK_items] PRIMARY KEY CLUSTERED ([id] ASC),\n    \
             CONSTRAINT [fk_parent] FOREIGN KEY ([code]) REFERENCES [dbo].[parents] ([code]) ON DELETE CASCADE\n);\n\
             CREATE NONCLUSTERED INDEX [ix_code] ON [dbo].[items] ([code] DESC);"
        );
    }

    #[test]
    fn sequences_render_their_options() {
        let definition = SequenceDefinition {
            type_name: "bigint".into(),
            start: "1".into(),
            increment: "5".into(),
            minimum: "1".into(),
            maximum: "100".into(),
            cycling: true,
            cache: Some(Some(10)),
        };
        let sequence = TableRef::new(None, Some("dbo".into()), "seq");
        assert_eq!(
            create_sequence(&sequence, &definition),
            "CREATE SEQUENCE [dbo].[seq] AS bigint START WITH 1 INCREMENT BY 5 \
             MINVALUE 1 MAXVALUE 100 CYCLE CACHE 10;"
        );
    }
}
