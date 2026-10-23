/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::ChildStderr;
use tokio::task::JoinHandle;

const TAIL_CAPACITY: usize = 8 * 1024;
const DRAIN_GRACE: Duration = Duration::from_millis(200);

pub(crate) struct StderrTail {
    buffer: Arc<Mutex<VecDeque<u8>>>,
    reader: Option<JoinHandle<()>>,
}

impl StderrTail {
    pub(crate) fn capture(stderr: Option<ChildStderr>) -> Self {
        let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(TAIL_CAPACITY)));
        let reader = stderr.map(|stderr| tokio::spawn(collect(stderr, Arc::clone(&buffer))));
        Self { buffer, reader }
    }

    pub(crate) async fn finish(mut self) -> Option<String> {
        if let Some(reader) = self.reader.take() {
            let _ = tokio::time::timeout(DRAIN_GRACE, reader).await;
        }
        let buffer = self.buffer.lock().unwrap_or_else(PoisonError::into_inner);
        let (front, back) = buffer.as_slices();
        let text = String::from_utf8_lossy(&[front, back].concat())
            .trim()
            .to_owned();
        (!text.is_empty()).then_some(text)
    }
}

impl Drop for StderrTail {
    fn drop(&mut self) {
        if let Some(reader) = &self.reader {
            reader.abort();
        }
    }
}

async fn collect(mut stderr: ChildStderr, buffer: Arc<Mutex<VecDeque<u8>>>) {
    let mut chunk = [0u8; 1024];
    while let Ok(read) = stderr.read(&mut chunk).await {
        if read == 0 {
            break;
        }
        let mut buffer = buffer.lock().unwrap_or_else(PoisonError::into_inner);
        buffer.extend(&chunk[..read]);
        let overflow = buffer.len().saturating_sub(TAIL_CAPACITY);
        buffer.drain(..overflow);
    }
}
