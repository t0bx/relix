/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::cancel::{OPAQUE_ID_HEADER, RunningRequests, cancel_server_tasks};
use crate::client::{RawResponse, SearchClient};
use crate::request::{ConsoleRequest, parse_request};
use crate::response_outcome::response_outcome;
use relix_core::StatementOutcome;
use relix_driver_api::{DriverError, ErrorKind, ExecuteOptions};
use relix_driver_http::header;

/// Runs one console request. A cancel resolves the call immediately and cancels the
/// server tasks tagged with the request's opaque id; a timed out request has its
/// server tasks cancelled as well, so nothing keeps running unseen.
pub async fn execute(
    client: &SearchClient,
    running: &RunningRequests,
    statement: &str,
    options: &ExecuteOptions,
) -> Result<StatementOutcome, DriverError> {
    let request = parse_request(statement)?;
    let ticket = running.begin();
    let result = tokio::select! {
        result = send(client, &request, &ticket.opaque_id) => result,
        () = ticket.abort.notified() => {
            Err(DriverError::new(ErrorKind::Cancelled, "the request was cancelled"))
        }
    };
    running.finish(&ticket);
    if let Err(error) = &result
        && error.kind == ErrorKind::Timeout
    {
        let _ = cancel_server_tasks(client, &ticket.opaque_id).await;
    }
    response_outcome(&request.method, result?, options.row_limit)
}

pub async fn cancel(client: &SearchClient, running: &RunningRequests) -> Result<(), DriverError> {
    let Some(ticket) = running.take() else {
        return Ok(());
    };
    ticket.abort.notify_one();
    let _ = cancel_server_tasks(client, &ticket.opaque_id).await;
    Ok(())
}

async fn send(
    client: &SearchClient,
    request: &ConsoleRequest,
    opaque_id: &str,
) -> Result<RawResponse, DriverError> {
    let mut builder = client
        .request(request.method.clone(), &request.path)?
        .header(OPAQUE_ID_HEADER, opaque_id);
    if let Some(body) = &request.body {
        builder = builder
            .header(header::CONTENT_TYPE, body.content_type)
            .body(body.text.clone());
    }
    client.exchange(builder).await
}
