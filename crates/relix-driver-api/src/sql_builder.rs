/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::{DriverError, ErrorKind};
use crate::sql_dialect_rules::{
    binary_literal, empty_insert_suffix, null_safe_operator, page_clause, qualifiers, typed_literal,
};
use relix_core::{CellChange, Dialect, RowChange, TableQuery, TableRef, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamStyle {
    Inline,
    QuestionMark,
    AtNumbered,
    ColonNumbered,
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
        qualifiers(self.dialect, table)
            .into_iter()
            .chain(std::iter::once(table.name.as_str()))
            .map(|part| self.dialect.quote_identifier(part))
            .collect::<Vec<_>>()
            .join(".")
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
        sql.push_str(&page_clause(
            self.dialect,
            !order.is_empty(),
            query.offset,
            u64::from(query.limit) + 1,
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
            writer.sql.push_str(empty_insert_suffix(self.dialect));
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

    pub fn inline_literal(&self, value: &Value) -> String {
        match value {
            Value::Null => "NULL".to_owned(),
            Value::Bytes(bytes) => binary_literal(self.dialect, bytes),
            other => typed_literal(self.dialect, other).unwrap_or_else(|| {
                let text = other.to_plain_string().unwrap_or_default();
                match self.dialect {
                    Dialect::MsSql => format!("N{}", self.dialect.quote_literal(&text)),
                    _ => self.dialect.quote_literal(&text),
                }
            }),
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
            ParamStyle::AtNumbered | ParamStyle::ColonNumbered => {
                self.params.push(value.clone());
                let prefix = match self.builder.style {
                    ParamStyle::AtNumbered => "@P",
                    _ => ":",
                };
                self.sql.push_str(&format!("{prefix}{}", self.params.len()));
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
    /// textual form so that types without an equality operator (json, xml) still work;
    /// numbers are compared by value instead, because the server may print floats in a
    /// different (e.g. exponent) notation than the client.
    fn push_null_safe_equals(&mut self, column: &str, value: &Value) {
        let quoted = self.builder.dialect.quote_identifier(column);
        let dialect = self.builder.dialect;
        match null_safe_operator(dialect) {
            Some(operator) if dialect == Dialect::Postgres && !value.is_numeric() => {
                self.sql.push_str(&format!("{quoted}::text{operator}"));
                self.push_value(value);
            }
            Some(operator) => {
                self.sql.push_str(&format!("{quoted}{operator}"));
                self.push_value(value);
            }
            None if value.is_null() => {
                self.sql.push_str(&format!("{quoted} IS NULL"));
            }
            None => {
                self.sql.push_str(&format!("{quoted} = "));
                self.push_value(value);
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
    fn postgres_numeric_guards_compare_by_value() {
        let builder = SqlBuilder::new(Dialect::Postgres, ParamStyle::Inline);
        let statement = builder
            .update(
                &users(),
                &[("id".into(), Value::Int(1))],
                &[CellChange {
                    column: "score".into(),
                    original: Value::Float(1e20),
                    updated: Value::Float(2.5),
                }],
            )
            .unwrap();
        assert!(
            statement
                .sql
                .ends_with("\"score\" IS NOT DISTINCT FROM '100000000000000000000'")
        );
    }

    #[test]
    fn missing_key_is_rejected_as_ambiguous() {
        let builder = SqlBuilder::new(Dialect::Postgres, ParamStyle::Inline);
        let error = builder.delete(&users(), &[]).unwrap_err();
        assert_eq!(error.kind, ErrorKind::AmbiguousRow);
    }

    #[test]
    fn mssql_pages_use_offset_fetch_and_numbered_params() {
        let builder = SqlBuilder::new(Dialect::MsSql, ParamStyle::AtNumbered);
        let table = TableRef::new(Some("shop".into()), Some("dbo".into()), "orders");
        assert_eq!(
            builder.select_page(&table, &TableQuery::default()),
            "SELECT * FROM [shop].[dbo].[orders] ORDER BY (SELECT NULL) \
             OFFSET 0 ROWS FETCH NEXT 201 ROWS ONLY"
        );
        let statement = builder
            .update(
                &table,
                &[("id".into(), Value::Int(1))],
                &[CellChange {
                    column: "note".into(),
                    original: Value::Null,
                    updated: Value::Text("x".into()),
                }],
            )
            .unwrap();
        assert_eq!(
            statement.sql,
            "UPDATE [shop].[dbo].[orders] SET [note] = @P1 WHERE [id] = @P2 AND [note] IS NULL"
        );
        assert_eq!(statement.params.len(), 2);
    }

    #[test]
    fn mssql_inline_strings_are_unicode_literals() {
        let builder = SqlBuilder::new(Dialect::MsSql, ParamStyle::Inline);
        assert_eq!(builder.inline_literal(&Value::Text("ä".into())), "N'ä'");
        assert_eq!(builder.inline_literal(&Value::Int(3)), "3");
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
