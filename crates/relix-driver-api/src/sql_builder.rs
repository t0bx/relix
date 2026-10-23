/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::{DriverError, ErrorKind};
use relix_core::value::hex_string;
use relix_core::{CellChange, Dialect, RowChange, TableQuery, TableRef, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamStyle {
    Inline,
    QuestionMark,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuiltStatement {
    pub sql: String,
    pub params: Vec<Value>,
}

#[derive(Debug, Clone, Copy)]
pub struct SqlBuilder {
    dialect: Dialect,
    style: ParamStyle,
}

impl SqlBuilder {
    pub fn new(dialect: Dialect, style: ParamStyle) -> Self {
        Self { dialect, style }
    }

    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    pub fn qualified_table(&self, table: &TableRef) -> String {
        let qualifier = match self.dialect {
            Dialect::Postgres => table.schema.as_deref(),
            Dialect::MySql => table.database.as_deref().or(table.schema.as_deref()),
            Dialect::Sqlite | Dialect::Generic => {
                table.schema.as_deref().or(table.database.as_deref())
            }
        };
        let name = self.dialect.quote_identifier(&table.name);
        match qualifier {
            Some(qualifier) => format!("{}.{name}", self.dialect.quote_identifier(qualifier)),
            None => name,
        }
    }

    /// Builds the page query. One row more than `limit` is requested so callers can tell
    /// whether another page exists without issuing a separate `COUNT`.
    pub fn select_page(&self, table: &TableRef, query: &TableQuery) -> String {
        let mut sql = format!("SELECT * FROM {}", self.qualified_table(table));
        if let Some(filter) = normalized_filter(query.filter.as_deref()) {
            sql.push_str(&format!(" WHERE ({filter})"));
        }
        let order = self.order_by(query);
        if !order.is_empty() {
            sql.push_str(" ORDER BY ");
            sql.push_str(&order.join(", "));
        }
        sql.push_str(&format!(
            " LIMIT {} OFFSET {}",
            u64::from(query.limit) + 1,
            query.offset
        ));
        sql
    }

    pub fn count(&self, table: &TableRef, filter: Option<&str>) -> String {
        let mut sql = format!("SELECT COUNT(*) FROM {}", self.qualified_table(table));
        if let Some(filter) = normalized_filter(filter) {
            sql.push_str(&format!(" WHERE ({filter})"));
        }
        sql
    }

    pub fn change_statement(
        &self,
        table: &TableRef,
        change: &RowChange,
    ) -> Result<BuiltStatement, DriverError> {
        match change {
            RowChange::Update { key, cells } => self.update(table, key, cells),
            RowChange::Insert { values } => Ok(self.insert(table, values)),
            RowChange::Delete { key } => self.delete(table, key),
        }
    }

    pub fn update(
        &self,
        table: &TableRef,
        key: &[(String, Value)],
        cells: &[CellChange],
    ) -> Result<BuiltStatement, DriverError> {
        ensure_key(key)?;
        if cells.is_empty() {
            return Err(DriverError::internal("update without changed cells"));
        }
        let mut writer = self.writer(format!("UPDATE {} SET ", self.qualified_table(table)));
        for (index, cell) in cells.iter().enumerate() {
            if index > 0 {
                writer.sql.push_str(", ");
            }
            writer.push_identifier(&cell.column);
            writer.sql.push_str(" = ");
            writer.push_value(&cell.updated);
        }
        writer.push_key_condition(key);
        for cell in cells {
            writer.sql.push_str(" AND ");
            writer.push_null_safe_equals(&cell.column, &cell.original);
        }
        Ok(writer.finish())
    }

    pub fn insert(&self, table: &TableRef, values: &[(String, Value)]) -> BuiltStatement {
        let mut writer = self.writer(format!("INSERT INTO {}", self.qualified_table(table)));
        if values.is_empty() {
            writer.sql.push_str(match self.dialect {
                Dialect::MySql => " () VALUES ()",
                _ => " DEFAULT VALUES",
            });
            return writer.finish();
        }
        writer.sql.push_str(" (");
        for (index, (column, _)) in values.iter().enumerate() {
            if index > 0 {
                writer.sql.push_str(", ");
            }
            writer.push_identifier(column);
        }
        writer.sql.push_str(") VALUES (");
        for (index, (_, value)) in values.iter().enumerate() {
            if index > 0 {
                writer.sql.push_str(", ");
            }
            writer.push_value(value);
        }
        writer.sql.push(')');
        writer.finish()
    }

    pub fn delete(
        &self,
        table: &TableRef,
        key: &[(String, Value)],
    ) -> Result<BuiltStatement, DriverError> {
        ensure_key(key)?;
        let mut writer = self.writer(format!("DELETE FROM {}", self.qualified_table(table)));
        writer.push_key_condition(key);
        Ok(writer.finish())
    }

    fn order_by(&self, query: &TableQuery) -> Vec<String> {
        let mut terms: Vec<String> = query
            .sort
            .iter()
            .map(|key| {
                format!(
                    "{} {}",
                    self.dialect.quote_identifier(&key.column),
                    key.direction.sql_keyword()
                )
            })
            .collect();
        for column in &query.tiebreaker {
            if !query.sort.iter().any(|key| &key.column == column) {
                terms.push(self.dialect.quote_identifier(column));
            }
        }
        terms
    }

    fn writer(&self, sql: String) -> StatementWriter {
        StatementWriter {
            builder: *self,
            sql,
            params: Vec::new(),
        }
    }

    fn inline_literal(&self, value: &Value) -> String {
        match value {
            Value::Null => "NULL".to_owned(),
            Value::Bytes(bytes) => match self.dialect {
                Dialect::Postgres => format!("'\\x{}'", &hex_string(bytes)[2..]),
                _ => format!("X'{}'", &hex_string(bytes)[2..]),
            },
            other => self
                .dialect
                .quote_literal(&other.to_plain_string().unwrap_or_default()),
        }
    }
}

struct StatementWriter {
    builder: SqlBuilder,
    sql: String,
    params: Vec<Value>,
}

impl StatementWriter {
    fn push_identifier(&mut self, identifier: &str) {
        let quoted = self.builder.dialect.quote_identifier(identifier);
        self.sql.push_str(&quoted);
    }

    fn push_value(&mut self, value: &Value) {
        match self.builder.style {
            ParamStyle::Inline => {
                let literal = self.builder.inline_literal(value);
                self.sql.push_str(&literal);
            }
            ParamStyle::QuestionMark => {
                self.sql.push('?');
                self.params.push(value.clone());
            }
        }
    }

    fn push_key_condition(&mut self, key: &[(String, Value)]) {
        self.sql.push_str(" WHERE ");
        for (index, (column, value)) in key.iter().enumerate() {
            if index > 0 {
                self.sql.push_str(" AND ");
            }
            if value.is_null() {
                self.push_null_safe_equals(column, value);
            } else {
                self.push_identifier(column);
                self.sql.push_str(" = ");
                self.push_value(value);
            }
        }
    }

    /// Emits a comparison that also treats two NULLs as equal. PostgreSQL compares the
    /// textual form so that types without an equality operator (json, xml) still work.
    fn push_null_safe_equals(&mut self, column: &str, value: &Value) {
        let quoted = self.builder.dialect.quote_identifier(column);
        match self.builder.dialect {
            Dialect::Postgres => {
                self.sql
                    .push_str(&format!("{quoted}::text IS NOT DISTINCT FROM "));
                self.push_value(value);
            }
            Dialect::MySql => {
                self.sql.push_str(&format!("{quoted} <=> "));
                self.push_value(value);
            }
            Dialect::Sqlite => {
                self.sql.push_str(&format!("{quoted} IS "));
                self.push_value(value);
            }
            Dialect::Generic => {
                self.sql.push_str(&format!("({quoted} = "));
                self.push_value(value);
                self.sql.push_str(&format!(" OR ({quoted} IS NULL AND "));
                self.push_value(value);
                self.sql.push_str(" IS NULL))");
            }
        }
    }

    fn finish(self) -> BuiltStatement {
        BuiltStatement {
            sql: self.sql,
            params: self.params,
        }
    }
}

fn normalized_filter(filter: Option<&str>) -> Option<&str> {
    let trimmed = filter?.trim().trim_end_matches(';').trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn ensure_key(key: &[(String, Value)]) -> Result<(), DriverError> {
    if key.is_empty() {
        return Err(DriverError::new(
            ErrorKind::AmbiguousRow,
            "row cannot be identified: table has no primary key or unique not-null index",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use relix_core::{SortDirection, SortKey};

    fn users() -> TableRef {
        TableRef::new(Some("app".into()), Some("public".into()), "users")
    }

    #[test]
    fn page_query_includes_filter_sort_tiebreaker_and_lookahead_row() {
        let query = TableQuery {
            filter: Some(" status = 'active'; ".into()),
            sort: vec![SortKey {
                column: "created_at".into(),
                direction: SortDirection::Descending,
            }],
            tiebreaker: vec!["id".into(), "created_at".into()],
            offset: 40,
            limit: 20,
        };
        let builder = SqlBuilder::new(Dialect::Postgres, ParamStyle::Inline);
        assert_eq!(
            builder.select_page(&users(), &query),
            "SELECT * FROM \"public\".\"users\" WHERE (status = 'active') \
             ORDER BY \"created_at\" DESC, \"id\" LIMIT 21 OFFSET 40"
        );
    }

    #[test]
    fn mysql_qualifies_with_database() {
        let builder = SqlBuilder::new(Dialect::MySql, ParamStyle::QuestionMark);
        let table = TableRef::new(Some("shop".into()), None, "orders");
        assert_eq!(builder.qualified_table(&table), "`shop`.`orders`");
        assert_eq!(
            builder.count(&table, Some("")),
            "SELECT COUNT(*) FROM `shop`.`orders`"
        );
    }

    #[test]
    fn postgres_update_inlines_literals_and_guards_original_values() {
        let builder = SqlBuilder::new(Dialect::Postgres, ParamStyle::Inline);
        let statement = builder
            .update(
                &users(),
                &[("id".into(), Value::Text("7".into()))],
                &[CellChange {
                    column: "name".into(),
                    original: Value::Text("O'Brien".into()),
                    updated: Value::Null,
                }],
            )
            .unwrap();
        assert_eq!(
            statement.sql,
            "UPDATE \"public\".\"users\" SET \"name\" = NULL WHERE \"id\" = '7' \
             AND \"name\"::text IS NOT DISTINCT FROM 'O''Brien'"
        );
        assert!(statement.params.is_empty());
    }

    #[test]
    fn positional_update_collects_params_in_order() {
        let builder = SqlBuilder::new(Dialect::Sqlite, ParamStyle::QuestionMark);
        let table = TableRef::new(Some("main".into()), None, "notes");
        let statement = builder
            .update(
                &table,
                &[("id".into(), Value::Int(1))],
                &[CellChange {
                    column: "body".into(),
                    original: Value::Null,
                    updated: Value::Text("x".into()),
                }],
            )
            .unwrap();
        assert_eq!(
            statement.sql,
            "UPDATE \"main\".\"notes\" SET \"body\" = ? WHERE \"id\" = ? AND \"body\" IS ?"
        );
        assert_eq!(
            statement.params,
            vec![Value::Text("x".into()), Value::Int(1), Value::Null]
        );
    }

    #[test]
    fn insert_and_delete_statements() {
        let builder = SqlBuilder::new(Dialect::MySql, ParamStyle::QuestionMark);
        let table = TableRef::new(Some("shop".into()), None, "orders");
        let insert = builder.insert(&table, &[("total".into(), Value::Int(5))]);
        assert_eq!(
            insert.sql,
            "INSERT INTO `shop`.`orders` (`total`) VALUES (?)"
        );
        assert_eq!(
            builder.insert(&table, &[]).sql,
            "INSERT INTO `shop`.`orders` () VALUES ()"
        );
        let delete = builder
            .delete(&table, &[("id".into(), Value::Int(3))])
            .unwrap();
        assert_eq!(delete.sql, "DELETE FROM `shop`.`orders` WHERE `id` = ?");
    }

    #[test]
    fn missing_key_is_rejected_as_ambiguous() {
        let builder = SqlBuilder::new(Dialect::Postgres, ParamStyle::Inline);
        let error = builder.delete(&users(), &[]).unwrap_err();
        assert_eq!(error.kind, ErrorKind::AmbiguousRow);
    }

    #[test]
    fn postgres_bytes_use_hex_escape_format() {
        let builder = SqlBuilder::new(Dialect::Postgres, ParamStyle::Inline);
        let insert = builder.insert(&users(), &[("blob".into(), Value::Bytes(vec![1, 255]))]);
        assert_eq!(
            insert.sql,
            "INSERT INTO \"public\".\"users\" (\"blob\") VALUES ('\\x01ff')"
        );
    }
}
