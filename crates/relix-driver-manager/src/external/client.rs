/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::line_reader::read_bounded_line;
use super::pending_calls::PendingCalls;
use super::stderr_tail::StderrTail;
use relix_driver_api::ipc::{IpcCall, IpcRequest, IpcResponse};
use relix_driver_api::{DriverError, ErrorKind};
use serde::de::DeserializeOwned;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::task::JoinHandle;

const MAX_RESPONSE_BYTES: usize = 256 * 1024 * 1024;

pub(crate) struct IpcClient {
    next_id: AtomicU64,
    writer: tokio::sync::Mutex<Option<ChildStdin>>,
    pending: Arc<PendingCalls>,
    child: Mutex<Option<Child>>,
    reader: JoinHandle<()>,
}

impl IpcClient {
    pub(crate) fn spawn(executable: &Path) -> Result<Self, DriverError> {
        let executable =
            std::path::absolute(executable).map_err(|error| start_error(executable, &error))?;
        let mut command = Command::new(&executable);
        if let Some(directory) = executable.parent() {
            command.current_dir(directory);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|error| start_error(&executable, &error))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| DriverError::internal("connector stdout is not piped"))?;
        let writer = child.stdin.take();
        let stderr = StderrTail::capture(child.stderr.take());
        let pending = Arc::new(PendingCalls::default());
        let reader = tokio::spawn(read_responses(stdout, Arc::clone(&pending), stderr));
        Ok(Self {
            next_id: AtomicU64::new(1),
            writer: tokio::sync::Mutex::new(writer),
            pending,
            child: Mutex::new(Some(child)),
            reader,
        })
    }

    pub(crate) async fn call<T: DeserializeOwned>(&self, call: IpcCall) -> Result<T, DriverError> {
        let method = call.method();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let receiver = self.pending.register(id)?;
        if let Err(error) = self.send(&IpcRequest { id, call }).await {
            self.pending.forget(id);
            return Err(error);
        }
        let value = match receiver.await {
            Ok(reply) => reply?,
            Err(_) => return Err(self.disconnected()),
        };
        serde_json::from_value(value).map_err(|error| {
            DriverError::new(
                ErrorKind::Protocol,
                format!("connector returned an invalid '{method}' result: {error}"),
            )
        })
    }

    /// Asks the connector to close, then closes its stdin and waits up to `grace` for the
    /// process to exit before killing it.
    pub(crate) async fn shutdown(&self, grace: Duration) -> Result<(), DriverError> {
        let closed = tokio::time::timeout(grace, self.call::<()>(IpcCall::Close {})).await;
        self.pending
            .fail_all(DriverError::connection("the connection is closed"));
        self.writer.lock().await.take();
        if let Some(mut child) = self.take_child()
            && tokio::time::timeout(grace, child.wait()).await.is_err()
        {
            let _ = child.kill().await;
        }
        match closed {
            Ok(Err(error)) if !error.kind.is_connection_loss() => Err(error),
            _ => Ok(()),
        }
    }

    pub(crate) async fn kill(&self) {
        self.pending
            .fail_all(DriverError::connection("the connection is closed"));
        if let Some(mut child) = self.take_child() {
            let _ = child.kill().await;
        }
    }

    async fn send(&self, request: &IpcRequest) -> Result<(), DriverError> {
        let mut line = serde_json::to_vec(request).map_err(|error| {
            DriverError::internal(format!("cannot encode connector request: {error}"))
        })?;
        line.push(b'\n');
        let mut writer = self.writer.lock().await;
        let Some(stdin) = writer.as_mut() else {
            return Err(self.disconnected());
        };
        let written = match stdin.write_all(&line).await {
            Ok(()) => stdin.flush().await,
            Err(error) => Err(error),
        };
        written.map_err(|_| self.disconnected())
    }

    fn disconnected(&self) -> DriverError {
        self.pending
            .failure()
            .unwrap_or_else(|| DriverError::connection("the connector process is not running"))
    }

    fn take_child(&self) -> Option<Child> {
        self.child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

impl Drop for IpcClient {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

async fn read_responses(stdout: ChildStdout, pending: Arc<PendingCalls>, stderr: StderrTail) {
    let mut reader = BufReader::new(stdout);
    let failure = loop {
        match read_bounded_line(&mut reader, MAX_RESPONSE_BYTES).await {
            Ok(Some(line)) if line.iter().all(u8::is_ascii_whitespace) => continue,
            Ok(Some(line)) => match serde_json::from_slice::<IpcResponse>(&line) {
                Ok(response) => pending.deliver(response),
                Err(error) => {
                    break DriverError::new(
                        ErrorKind::Protocol,
                        format!("connector wrote malformed output: {error}"),
                    );
                }
            },
            Ok(None) => {
                let exited = DriverError::connection("the connector process exited");
                break match stderr.finish().await {
                    Some(output) => exited.with_detail(output),
                    None => exited,
                };
            }
            Err(error) => break error,
        }
    };
    pending.fail_all(failure);
}

fn start_error(executable: &Path, error: &std::io::Error) -> DriverError {
    DriverError::connection(format!(
        "cannot start connector {}: {error}",
        executable.display()
    ))
}
