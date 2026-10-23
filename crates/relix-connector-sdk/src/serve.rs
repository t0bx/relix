/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::dispatch::dispatch;
use crate::session::Session;
use relix_driver_api::ipc::{IpcCall, IpcRequest, IpcResponse};
use relix_driver_api::{DatabaseDriver, DriverError, ErrorKind};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("connector I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Serves `driver` over the process's stdin and stdout until RELIX closes the connection
/// or stdin reaches end of input.
pub async fn serve(driver: Arc<dyn DatabaseDriver>) -> Result<(), ServeError> {
    serve_io(driver, tokio::io::stdin(), tokio::io::stdout()).await
}

/// Reads one request per line from `input` and handles every request on its own task, so
/// a `cancel` is processed while an `execute` is still running. Responses are written by a
/// single writer task, one JSON document per line, in completion order. A `close` request
/// closes the connection right away (interrupting running statements where the driver
/// supports it); the function returns once every request has been answered.
pub async fn serve_io<R, W>(
    driver: Arc<dyn DatabaseDriver>,
    input: R,
    output: W,
) -> Result<(), ServeError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let session = Arc::new(Session::default());
    let (sender, receiver) = mpsc::unbounded_channel::<IpcResponse>();
    let writer = tokio::spawn(write_responses(output, receiver));
    let mut lines = BufReader::new(input).lines();
    let mut handlers = JoinSet::new();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let request = match parse_request(&line) {
            Ok(request) => request,
            Err(response) => {
                let _ = sender.send(response);
                continue;
            }
        };
        if matches!(request.call, IpcCall::Close {}) {
            let response = respond(driver.as_ref(), &session, request).await;
            let _ = sender.send(response);
            break;
        }
        let driver = Arc::clone(&driver);
        let session = Arc::clone(&session);
        let sender = sender.clone();
        handlers.spawn(async move {
            let response = respond(driver.as_ref(), &session, request).await;
            let _ = sender.send(response);
        });
        while handlers.try_join_next().is_some() {}
    }
    while handlers.join_next().await.is_some() {}
    if let Some(connection) = session.take() {
        let _ = connection.close().await;
    }
    drop(sender);
    writer
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?
}

async fn respond(
    driver: &dyn DatabaseDriver,
    session: &Session,
    request: IpcRequest,
) -> IpcResponse {
    match dispatch(driver, session, request.call).await {
        Ok(result) => IpcResponse::success(request.id, result),
        Err(error) => IpcResponse::failure(request.id, error),
    }
}

/// Decodes a request line. Requests with an unknown method or malformed parameters are
/// answered with a protocol error under their id when it can be recovered (id 0 otherwise).
fn parse_request(line: &str) -> Result<IpcRequest, IpcResponse> {
    serde_json::from_str::<IpcRequest>(line).map_err(|error| {
        let id = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|value| value.get("id").and_then(serde_json::Value::as_u64))
            .unwrap_or_default();
        IpcResponse::failure(
            id,
            DriverError::new(ErrorKind::Protocol, format!("invalid request: {error}")),
        )
    })
}

async fn write_responses<W: AsyncWrite + Unpin>(
    mut output: W,
    mut responses: mpsc::UnboundedReceiver<IpcResponse>,
) -> Result<(), ServeError> {
    while let Some(response) = responses.recv().await {
        let mut line = serde_json::to_vec(&response).map_err(std::io::Error::other)?;
        line.push(b'\n');
        output.write_all(&line).await?;
        output.flush().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use relix_core::ConnectionConfig;
    use relix_driver_api::{
        ConnectionTarget, DatabaseConnection, DriverCapabilities, DriverMetadata,
    };
    use serde_json::{Value, json};
    use tokio::io::AsyncReadExt;

    struct RefusingDriver;

    #[async_trait]
    impl DatabaseDriver for RefusingDriver {
        fn metadata(&self) -> DriverMetadata {
            DriverMetadata::built_in("refusing", "Refusing", None, ConnectionTarget::File)
        }

        fn capabilities(&self) -> DriverCapabilities {
            DriverCapabilities::relational()
        }

        async fn connect(
            &self,
            _config: &ConnectionConfig,
        ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
            Err(DriverError::new(ErrorKind::Authentication, "denied"))
        }
    }

    async fn exchange(requests: &[Value]) -> Vec<Value> {
        let input: String = requests
            .iter()
            .map(|request| format!("{request}\n"))
            .collect();
        let (writer, mut reader) = tokio::io::duplex(64 * 1024);
        serve_io(Arc::new(RefusingDriver), input.as_bytes(), writer)
            .await
            .unwrap();
        let mut output = String::new();
        reader.read_to_string(&mut output).await.unwrap();
        let mut responses: Vec<Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        responses.sort_by_key(|response| response["id"].as_u64());
        responses
    }

    #[tokio::test]
    async fn answers_every_request_by_id() {
        let responses = exchange(&[
            json!({"id": 1, "method": "handshake", "params": {"protocol_version": 1}}),
            json!({"id": 2, "method": "teleport", "params": {}}),
            json!({"id": 3, "method": "databases", "params": {}}),
            json!({"id": 4, "method": "connect", "params": {"settings": {}, "password": "pw"}}),
            json!({"id": 5, "method": "handshake", "params": {"protocol_version": 99}}),
        ])
        .await;
        assert_eq!(responses.len(), 5);
        assert_eq!(responses[0]["result"]["metadata"]["id"], "refusing");
        assert_eq!(responses[0]["result"]["protocol_version"], 1);
        assert_eq!(responses[1]["error"]["kind"], "protocol");
        assert_eq!(responses[2]["error"]["kind"], "connection");
        assert_eq!(responses[3]["error"]["kind"], "authentication");
        assert_eq!(responses[4]["error"]["kind"], "protocol");
    }

    #[tokio::test]
    async fn close_ends_the_session() {
        let responses = exchange(&[
            json!({"id": 1, "method": "close", "params": {}}),
            json!({"id": 2, "method": "ping", "params": {}}),
        ])
        .await;
        assert_eq!(responses, vec![json!({"id": 1, "result": null})]);
    }
}
