/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::attribute_json::{Item, attribute_to_json};
use crate::connection::DynamoConnection;
use crate::errors::map_sdk_error;
use async_trait::async_trait;
use aws_sdk_dynamodb::Client;
use relix_core::StatementOutcome;
use relix_driver_api::{DriverError, ErrorKind, ExecuteOptions, QueryExecutor, document_rows};
use serde_json::Value as Json;

const WRITE_KEYWORDS: [&str; 3] = ["INSERT", "UPDATE", "DELETE"];

#[async_trait]
impl QueryExecutor for DynamoConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let statement = statement.trim().trim_end_matches(';').trim_end();
        if statement.is_empty() {
            return Err(DriverError::query("empty statement"));
        }
        let client = self.client()?;
        let aborted = self.abort.begin();
        let outcome = tokio::select! {
            outcome = run_statement(&client, statement, options.row_limit) => outcome,
            _ = aborted.notified() => {
                Err(DriverError::new(ErrorKind::Cancelled, "the statement was cancelled"))
            }
        };
        self.abort.finish();
        outcome
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        self.abort.abort();
        Ok(())
    }
}

/// Runs one PartiQL statement with `ExecuteStatement`. Reads follow `NextToken` until
/// the row limit is reached (requesting one item more than needed to detect further
/// rows); INSERT, UPDATE and DELETE always address exactly one item.
async fn run_statement(
    client: &Client,
    statement: &str,
    row_limit: usize,
) -> Result<StatementOutcome, DriverError> {
    if is_write_statement(statement) {
        client
            .execute_statement()
            .statement(statement)
            .send()
            .await
            .map_err(map_sdk_error)?;
        return Ok(StatementOutcome::Affected { rows: 1 });
    }
    let mut items: Vec<Item> = Vec::new();
    let mut next_token = None;
    loop {
        let wanted = row_limit.saturating_sub(items.len()).saturating_add(1);
        let output = client
            .execute_statement()
            .statement(statement)
            .set_next_token(next_token)
            .limit(i32::try_from(wanted).unwrap_or(i32::MAX))
            .send()
            .await
            .map_err(map_sdk_error)?;
        items.extend(output.items.unwrap_or_default());
        next_token = output.next_token;
        if items.len() > row_limit || next_token.is_none() {
            break;
        }
    }
    let has_more = items.len() > row_limit || next_token.is_some();
    items.truncate(row_limit);
    Ok(StatementOutcome::Rows(document_rows(
        items.iter().map(sorted_fields),
        has_more,
    )))
}

fn is_write_statement(statement: &str) -> bool {
    statement.split_whitespace().next().is_some_and(|first| {
        WRITE_KEYWORDS
            .iter()
            .any(|keyword| keyword.eq_ignore_ascii_case(first))
    })
}

fn sorted_fields(item: &Item) -> Vec<(String, Json)> {
    let mut fields: Vec<(String, Json)> = item
        .iter()
        .map(|(name, value)| (name.clone(), attribute_to_json(value)))
        .collect();
    fields.sort_by(|left, right| left.0.cmp(&right.0));
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::types::AttributeValue;

    #[test]
    fn write_statements_are_recognised() {
        assert!(is_write_statement("insert into t value {'pk': 'a'}"));
        assert!(is_write_statement("  DELETE FROM t WHERE pk = 'a'"));
        assert!(!is_write_statement("SELECT * FROM t"));
    }

    #[test]
    fn row_fields_are_sorted_by_name() {
        let item = Item::from([
            ("z".to_owned(), AttributeValue::N("1".into())),
            ("a".to_owned(), AttributeValue::S("x".into())),
        ]);
        let names: Vec<_> = sorted_fields(&item)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["a", "z"]);
    }
}
