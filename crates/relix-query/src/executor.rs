/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::StatementOutcome;
use relix_driver_api::{DatabaseConnection, DriverError, ErrorKind, ExecuteOptions, QueryExecutor};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const CANCEL_GRACE_PERIOD: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct ScriptRequest {
    pub statements: Vec<String>,
    pub options: ExecuteOptions,
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct StatementReport {
    pub index: usize,
    pub statement: String,
    pub result: Result<StatementOutcome, DriverError>,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptOutcome {
    Completed,
    Failed { index: usize },
    Cancelled { index: usize },
}

/// Runs statements one after another on the connection's session, reporting each result
/// as soon as it is available. Execution stops at the first failure so later statements
/// never run against an unexpected state. Cancellation and timeouts ask the driver to
/// cancel the running statement and then wait briefly for it to actually stop.
pub async fn run_script<F>(
    connection: Arc<dyn DatabaseConnection>,
    request: ScriptRequest,
    cancel: CancellationToken,
    mut on_report: F,
) -> ScriptOutcome
where
    F: FnMut(StatementReport),
{
    let Some(executor) = connection.as_query() else {
        on_report(StatementReport {
            index: 0,
            statement: request.statements.first().cloned().unwrap_or_default(),
            result: Err(DriverError::unsupported(
                "this data source has no query console",
            )),
            elapsed: Duration::ZERO,
        });
        return ScriptOutcome::Failed { index: 0 };
    };
    for (index, statement) in request.statements.iter().enumerate() {
        if cancel.is_cancelled() {
            return ScriptOutcome::Cancelled { index };
        }
        let started = Instant::now();
        let result = execute_one(executor, statement, &request, &cancel).await;
        let failed = result.as_ref().err().map(|error| error.kind);
        on_report(StatementReport {
            index,
            statement: statement.clone(),
            result,
            elapsed: started.elapsed(),
        });
        match failed {
            Some(ErrorKind::Cancelled) => return ScriptOutcome::Cancelled { index },
            Some(_) => return ScriptOutcome::Failed { index },
            None => {}
        }
    }
    ScriptOutcome::Completed
}

async fn execute_one(
    sql: &dyn QueryExecutor,
    statement: &str,
    request: &ScriptRequest,
    cancel: &CancellationToken,
) -> Result<StatementOutcome, DriverError> {
    let execution = sql.execute(statement, &request.options);
    tokio::pin!(execution);
    let deadline = async {
        match request.timeout {
            Some(timeout) => tokio::time::sleep(timeout).await,
            None => std::future::pending().await,
        }
    };
    let interruption = tokio::select! {
        result = &mut execution => return result,
        () = cancel.cancelled() => DriverError::new(ErrorKind::Cancelled, "query cancelled by user"),
        () = deadline => DriverError::new(ErrorKind::Timeout, "query exceeded the configured timeout"),
    };
    let _ = sql.cancel().await;
    match tokio::time::timeout(CANCEL_GRACE_PERIOD, &mut execution).await {
        Ok(Ok(outcome)) => Ok(outcome),
        Ok(Err(_)) | Err(_) => Err(interruption),
    }
}
