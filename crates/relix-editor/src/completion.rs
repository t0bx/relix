/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::{CatalogTable, CompletionCatalog};
use crate::context::{CompletionContext, TableReference, completion_context, table_references};
use relix_core::Dialect;
use relix_query::splitter::statement_at;
use relix_query::vocabulary;

const MAX_ITEMS: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompletionKind {
    Column,
    Alias,
    Table,
    View,
    Schema,
    Function,
    DataType,
    Keyword,
    Snippet,
}

impl CompletionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Column => "col",
            Self::Alias => "alias",
            Self::Table => "table",
            Self::View => "view",
            Self::Schema => "schema",
            Self::Function => "fn",
            Self::DataType => "type",
            Self::Keyword => "kw",
            Self::Snippet => "hist",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub insert_text: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    score: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompletionResult {
    pub replace_start: usize,
    pub prefix: String,
    pub items: Vec<CompletionItem>,
    pub missing_tables: Vec<(Option<String>, String)>,
}

/// Computes completion candidates for the cursor at byte `offset` of `source`.
///
/// The current statement is analysed for its context (table position, `alias.` member
/// access, expression, type) and the tables it references. Candidates come from live
/// metadata in `catalog`, dialect keywords/functions/types and previously executed
/// statements. Referenced tables whose columns are not loaded yet are reported in
/// `missing_tables` so the caller can fetch them asynchronously and ask again.
pub fn complete(catalog: &CompletionCatalog, source: &str, offset: usize) -> CompletionResult {
    let offset = floor_char_boundary(source, offset);
    let prefix_start = source[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, ch)| ch.is_alphanumeric() || *ch == '_')
        .last()
        .map_or(offset, |(index, _)| index);
    let prefix = &source[prefix_start..offset];
    let dialect = catalog.dialect;
    let statement = statement_at(source, dialect, offset);
    let (statement_text, statement_start) = statement
        .as_ref()
        .filter(|statement| statement.start <= prefix_start)
        .map_or(("", prefix_start), |statement| {
            (statement.text.as_str(), statement.start)
        });
    let local_prefix_end = prefix_start - statement_start;
    let before = statement_text.get(..local_prefix_end).unwrap_or_default();
    let context = completion_context(before, dialect);
    let references = table_references(statement_text, dialect);
    let mut collector = Collector::new(prefix, dialect);
    match &context {
        CompletionContext::Member { qualifier } => {
            collector.member(catalog, &references, qualifier);
        }
        CompletionContext::TableName => collector.tables(catalog),
        CompletionContext::DataType => {
            collector.words(vocabulary::data_types(dialect), CompletionKind::DataType)
        }
        CompletionContext::Expression => {
            collector.columns_of(catalog, &references);
            collector.aliases(&references);
            collector.functions(catalog);
            collector.words(vocabulary::keywords(dialect), CompletionKind::Keyword);
        }
        CompletionContext::StatementStart => {
            collector.words(vocabulary::keywords(dialect), CompletionKind::Keyword);
            collector.fragments(catalog);
        }
    }
    let show_without_prefix = matches!(
        context,
        CompletionContext::Member { .. }
            | CompletionContext::TableName
            | CompletionContext::DataType
    );
    let missing_tables = std::mem::take(&mut collector.missing);
    let items = if prefix.is_empty() && !show_without_prefix {
        Vec::new()
    } else {
        collector.finish()
    };
    CompletionResult {
        replace_start: prefix_start,
        prefix: prefix.to_owned(),
        items,
        missing_tables,
    }
}

fn floor_char_boundary(source: &str, offset: usize) -> usize {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

struct Collector<'a> {
    prefix: &'a str,
    dialect: Dialect,
    items: Vec<CompletionItem>,
    missing: Vec<(Option<String>, String)>,
}

impl<'a> Collector<'a> {
    fn new(prefix: &'a str, dialect: Dialect) -> Self {
        Self {
            prefix,
            dialect,
            items: Vec::new(),
            missing: Vec::new(),
        }
    }

    fn push(
        &mut self,
        label: &str,
        insert_text: String,
        kind: CompletionKind,
        detail: Option<String>,
    ) {
        let Some(score) = match_score(self.prefix, label) else {
            return;
        };
        if self
            .items
            .iter()
            .any(|item| item.label == label && item.kind == kind)
        {
            return;
        }
        self.items.push(CompletionItem {
            label: label.to_owned(),
            insert_text,
            kind,
            detail,
            score: score + kind_bonus(kind),
        });
    }

    fn identifier(&self, name: &str) -> String {
        if needs_quoting(name, self.dialect) {
            self.dialect.quote_identifier(name)
        } else {
            name.to_owned()
        }
    }

    fn member(
        &mut self,
        catalog: &CompletionCatalog,
        references: &[TableReference],
        qualifier: &str,
    ) {
        let referenced = references.iter().find(|reference| {
            reference
                .alias
                .as_deref()
                .is_some_and(|alias| alias.eq_ignore_ascii_case(qualifier))
                || reference.name.eq_ignore_ascii_case(qualifier)
        });
        let table = match referenced {
            Some(reference) => catalog.find_table(reference.schema.as_deref(), &reference.name),
            None => catalog.find_table(None, qualifier),
        };
        if let Some(table) = table {
            self.table_columns(table, None);
        }
        if catalog.has_schema(qualifier) {
            let tables: Vec<&CatalogTable> = catalog
                .tables
                .iter()
                .filter(|table| {
                    table
                        .schema
                        .as_deref()
                        .is_some_and(|schema| schema.eq_ignore_ascii_case(qualifier))
                })
                .collect();
            for table in tables {
                self.table_entry(table, false);
            }
        }
    }

    fn table_columns(&mut self, table: &CatalogTable, owner: Option<&str>) {
        match &table.columns {
            Some(columns) => {
                for column in columns {
                    let detail = match owner {
                        Some(owner) => format!("{} · {owner}", column.data_type),
                        None => column.data_type.clone(),
                    };
                    let insert = self.identifier(&column.name);
                    self.push(&column.name, insert, CompletionKind::Column, Some(detail));
                }
            }
            None => self
                .missing
                .push((table.schema.clone(), table.name.clone())),
        }
    }

    fn columns_of(&mut self, catalog: &CompletionCatalog, references: &[TableReference]) {
        for reference in references {
            if let Some(table) = catalog.find_table(reference.schema.as_deref(), &reference.name) {
                let owner = reference.alias.as_deref().unwrap_or(&table.name);
                self.table_columns(table, Some(owner));
            }
        }
    }

    fn aliases(&mut self, references: &[TableReference]) {
        for reference in references {
            if let Some(alias) = &reference.alias {
                self.push(
                    alias,
                    alias.clone(),
                    CompletionKind::Alias,
                    Some(reference.name.clone()),
                );
            }
        }
    }

    fn tables(&mut self, catalog: &CompletionCatalog) {
        for table in &catalog.tables {
            let qualify = !catalog.is_default_schema(table.schema.as_deref());
            self.table_entry(table, qualify);
        }
        for schema in &catalog.schemas {
            let insert = self.identifier(schema);
            self.push(schema, insert, CompletionKind::Schema, None);
        }
    }

    fn table_entry(&mut self, table: &CatalogTable, qualify: bool) {
        let kind = if table.kind.is_relation() && table.kind != relix_core::ObjectKind::Table {
            CompletionKind::View
        } else {
            CompletionKind::Table
        };
        let insert = match (&table.schema, qualify) {
            (Some(schema), true) => format!(
                "{}.{}",
                self.identifier(schema),
                self.identifier(&table.name)
            ),
            _ => self.identifier(&table.name),
        };
        self.push(&table.name, insert, kind, table.schema.clone());
    }

    fn functions(&mut self, catalog: &CompletionCatalog) {
        for routine in &catalog.routines {
            self.push(
                routine,
                format!("{routine}("),
                CompletionKind::Function,
                Some("routine".into()),
            );
        }
        for function in vocabulary::functions(self.dialect) {
            self.push(
                function,
                format!("{function}("),
                CompletionKind::Function,
                None,
            );
        }
    }

    fn words(&mut self, words: impl Iterator<Item = &'static str>, kind: CompletionKind) {
        for word in words {
            self.push(word, word.to_owned(), kind, None);
        }
    }

    fn fragments(&mut self, catalog: &CompletionCatalog) {
        for fragment in &catalog.fragments {
            let first_line = fragment.lines().next().unwrap_or_default();
            if self.prefix.is_empty()
                || !first_line
                    .to_lowercase()
                    .starts_with(&self.prefix.to_lowercase())
            {
                continue;
            }
            self.push(
                first_line,
                fragment.clone(),
                CompletionKind::Snippet,
                Some("history".into()),
            );
        }
    }

    fn finish(mut self) -> Vec<CompletionItem> {
        self.items.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.kind.cmp(&right.kind))
                .then_with(|| left.label.len().cmp(&right.label.len()))
                .then_with(|| left.label.cmp(&right.label))
        });
        self.items.truncate(MAX_ITEMS);
        self.items
    }
}

fn kind_bonus(kind: CompletionKind) -> u32 {
    match kind {
        CompletionKind::Column | CompletionKind::Alias => 30,
        CompletionKind::Table | CompletionKind::View => 25,
        CompletionKind::Schema => 15,
        CompletionKind::Function | CompletionKind::DataType => 10,
        CompletionKind::Snippet => 5,
        CompletionKind::Keyword => 0,
    }
}

/// Scores how well `candidate` matches the typed `prefix`: exact prefix matches rank
/// highest, then substring matches, then in-order subsequence (fuzzy) matches.
pub fn match_score(prefix: &str, candidate: &str) -> Option<u32> {
    if prefix.is_empty() {
        return Some(100);
    }
    let prefix = prefix.to_lowercase();
    let lower = candidate.to_lowercase();
    if lower == prefix {
        return Some(400);
    }
    if lower.starts_with(&prefix) {
        return Some(300);
    }
    if lower.contains(&prefix) {
        return Some(200);
    }
    let mut chars = lower.chars();
    prefix
        .chars()
        .all(|wanted| chars.any(|ch| ch == wanted))
        .then_some(100)
}

fn needs_quoting(name: &str, dialect: Dialect) -> bool {
    let mut chars = name.chars();
    let valid_start = chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_');
    let valid_rest = name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$');
    let case_sensitive =
        dialect == Dialect::Postgres && name.chars().any(|ch| ch.is_ascii_uppercase());
    !valid_start || !valid_rest || case_sensitive || vocabulary::is_keyword(dialect, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CatalogColumn;
    use relix_core::ObjectKind;

    fn catalog() -> CompletionCatalog {
        let column = |name: &str| CatalogColumn {
            name: name.into(),
            data_type: "text".into(),
        };
        CompletionCatalog {
            dialect: Dialect::Postgres,
            default_schema: Some("public".into()),
            schemas: vec!["public".into(), "audit".into()],
            tables: vec![
                CatalogTable {
                    schema: Some("public".into()),
                    name: "users".into(),
                    kind: ObjectKind::Table,
                    columns: Some(vec![column("id"), column("username"), column("Status")]),
                },
                CatalogTable {
                    schema: Some("public".into()),
                    name: "orders".into(),
                    kind: ObjectKind::Table,
                    columns: None,
                },
                CatalogTable {
                    schema: Some("audit".into()),
                    name: "user_events".into(),
                    kind: ObjectKind::View,
                    columns: None,
                },
            ],
            routines: vec!["refresh_stats".into()],
            fragments: vec!["SELECT count(*) FROM users".into()],
        }
    }

    fn labels(source: &str) -> Vec<String> {
        complete(&catalog(), source, source.len())
            .items
            .into_iter()
            .map(|item| item.label)
            .collect()
    }

    #[test]
    fn suggests_tables_after_from() {
        let result = complete(&catalog(), "SELECT * FROM us", 16);
        assert_eq!(result.prefix, "us");
        assert_eq!(result.replace_start, 14);
        assert_eq!(result.items[0].label, "users");
        let event = result
            .items
            .iter()
            .find(|item| item.label == "user_events")
            .unwrap();
        assert_eq!(event.insert_text, "audit.user_events");
        assert_eq!(event.kind, CompletionKind::View);
    }

    #[test]
    fn suggests_columns_through_aliases() {
        let source = "SELECT u. FROM users u";
        let result = complete(&catalog(), source, 9);
        let names: Vec<_> = result
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(names, vec!["id", "Status", "username"]);
        let status = result
            .items
            .iter()
            .find(|item| item.label == "Status")
            .unwrap();
        assert_eq!(status.insert_text, "\"Status\"");
    }

    #[test]
    fn expression_context_mixes_columns_functions_and_keywords() {
        let source = "SELECT * FROM users WHERE use";
        let items = labels(source);
        assert_eq!(items[0], "username");
        assert!(
            complete(&catalog(), "SELECT co", 9)
                .items
                .iter()
                .any(|item| item.label == "COUNT")
        );
        assert!(
            complete(&catalog(), "SELECT ref", 10)
                .items
                .iter()
                .any(|item| item.kind == CompletionKind::Function && item.label == "refresh_stats")
        );
    }

    #[test]
    fn reports_missing_column_metadata() {
        let source = "SELECT o. FROM orders o";
        let result = complete(&catalog(), source, 9);
        assert_eq!(
            result.missing_tables,
            vec![(Some("public".into()), "orders".into())]
        );
    }

    #[test]
    fn schema_member_lists_its_tables() {
        assert_eq!(labels("SELECT * FROM audit."), vec!["user_events"]);
    }

    #[test]
    fn statement_start_offers_keywords_and_history() {
        let items = complete(&catalog(), "sel", 3).items;
        assert_eq!(items[0].label, "SELECT count(*) FROM users");
        assert!(items.iter().any(|item| item.label == "SELECT"));
        assert!(complete(&catalog(), "", 0).items.is_empty());
    }

    #[test]
    fn data_types_after_cast_operator() {
        assert!(labels("SELECT x::js").contains(&"JSONB".to_owned()));
    }

    #[test]
    fn fuzzy_matching() {
        assert_eq!(match_score("usr", "users"), Some(100));
        assert_eq!(match_score("xyz", "users"), None);
        assert!(match_score("us", "users") > match_score("se", "users"));
    }
}
