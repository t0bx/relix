/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::cancellation::{kill_tagged_operations, operation_tag};
use crate::command_reply::reply_outcome;
use crate::connection::MongoConnection;
use crate::errors::map_error;
use crate::extended_json::to_display_fields;
use crate::statement::{ConsoleStatement, parse_statement};
use async_trait::async_trait;
use mongodb::Database;
use mongodb::bson::{Bson, Document, doc};
use relix_core::StatementOutcome;
use relix_driver_api::{DriverError, ErrorKind, ExecuteOptions, QueryExecutor, document_rows};

const CURSOR_COMMANDS: [&str; 4] = ["find", "aggregate", "listCollections", "listIndexes"];
const COMMENT_FIELD: &str = "comment";

#[async_trait]
impl QueryExecutor for MongoConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        match parse_statement(statement)? {
            ConsoleStatement::Use(database) => {
                self.use_database(&database);
                Ok(StatementOutcome::Completed {
                    message: format!("switched to database {database}"),
                })
            }
            ConsoleStatement::Command { database, command } => {
                let database = database.unwrap_or_else(|| self.current_database());
                self.run_tracked(&database, command, options.row_limit)
                    .await
            }
        }
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        let Some(running) = self.tracker.current() else {
            return Ok(());
        };
        running.abort.notify_one();
        match running.tag {
            Some(tag) => kill_tagged_operations(&self.client()?, &tag)
                .await
                .map(|_| ()),
            None => Ok(()),
        }
    }
}

impl MongoConnection {
    /// Runs a console command while it can be cancelled: the command is tagged with a
    /// unique `comment` (unless it already has one) so `cancel` can find and kill it on
    /// the server, and the local wait is abandoned as soon as cancellation is requested.
    async fn run_tracked(
        &self,
        database: &str,
        mut command: Document,
        row_limit: usize,
    ) -> Result<StatementOutcome, DriverError> {
        let tag = match command.get(COMMENT_FIELD) {
            Some(existing) => Some(existing.clone()),
            None if self.supports_comment => {
                let tag = operation_tag();
                command.insert(COMMENT_FIELD, tag.clone());
                Some(tag)
            }
            None => None,
        };
        let database = self.database(database)?;
        let running = self.tracker.begin(tag.clone());
        let outcome = tokio::select! {
            outcome = run_command(&database, command, row_limit, tag) => outcome,
            _ = running.abort.notified() => {
                Err(DriverError::new(ErrorKind::Cancelled, "the command was cancelled"))
            }
        };
        self.tracker.finish();
        outcome
    }
}

async fn run_command(
    database: &Database,
    mut command: Document,
    row_limit: usize,
    tag: Option<Bson>,
) -> Result<StatementOutcome, DriverError> {
    let name = command.keys().next().cloned().unwrap_or_default();
    if !returns_cursor(&name, &command) {
        let reply = database.run_command(command).await.map_err(map_error)?;
        return reply_outcome(&name, reply, row_limit);
    }
    if name == "aggregate" && !command.contains_key("cursor") {
        command.insert("cursor", doc! {});
    }
    let batch_size = u32::try_from(row_limit.saturating_add(1)).unwrap_or(u32::MAX);
    let mut action = database.run_cursor_command(command).batch_size(batch_size);
    if let Some(tag) = tag {
        action = action.comment(tag);
    }
    let mut cursor = action.await.map_err(map_error)?;
    let mut rows = Vec::new();
    while rows.len() < row_limit && cursor.advance().await.map_err(map_error)? {
        let document: Document = cursor.deserialize_current().map_err(map_error)?;
        rows.push(to_display_fields(document));
    }
    let has_more = rows.len() >= row_limit && cursor.advance().await.map_err(map_error)?;
    Ok(StatementOutcome::Rows(document_rows(rows, has_more)))
}

fn returns_cursor(name: &str, command: &Document) -> bool {
    CURSOR_COMMANDS.contains(&name) && !command.get_bool("explain").unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_commands_are_recognised_unless_explained() {
        assert!(returns_cursor("find", &doc! { "find": "users" }));
        assert!(returns_cursor(
            "aggregate",
            &doc! { "aggregate": "o", "pipeline": [] }
        ));
        assert!(!returns_cursor(
            "aggregate",
            &doc! { "aggregate": "o", "pipeline": [], "explain": true }
        ));
        assert!(!returns_cursor("count", &doc! { "count": "users" }));
    }
}
