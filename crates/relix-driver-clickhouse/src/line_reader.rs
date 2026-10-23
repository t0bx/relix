/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::DriverError;
use relix_driver_http::{Response, map_transport_error};

/// Splits a streamed response body into lines as chunks arrive, without buffering more
/// than the current, incomplete line.
pub(crate) struct LineReader {
    response: Response,
    buffer: Vec<u8>,
    start: usize,
    finished: bool,
}

impl LineReader {
    pub(crate) fn new(response: Response) -> Self {
        Self {
            response,
            buffer: Vec::new(),
            start: 0,
            finished: false,
        }
    }

    pub(crate) async fn next_line(&mut self) -> Result<Option<String>, DriverError> {
        loop {
            if let Some(offset) = self.buffer[self.start..]
                .iter()
                .position(|&byte| byte == b'\n')
            {
                let end = self.start + offset;
                let line = decode(&self.buffer[self.start..end]);
                self.start = end + 1;
                return Ok(Some(line));
            }
            if self.finished {
                if self.start == self.buffer.len() {
                    return Ok(None);
                }
                let line = decode(&self.buffer[self.start..]);
                self.start = self.buffer.len();
                return Ok(Some(line));
            }
            self.buffer.drain(..self.start);
            self.start = 0;
            match self
                .response
                .chunk()
                .await
                .map_err(|error| map_transport_error(&error))?
            {
                Some(chunk) => self.buffer.extend_from_slice(&chunk),
                None => self.finished = true,
            }
        }
    }
}

fn decode(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.strip_suffix('\r').unwrap_or(&text).to_owned()
}
