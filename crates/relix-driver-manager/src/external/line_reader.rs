/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// Reads one newline-terminated line without the terminator, failing once it grows beyond
/// `max_bytes`. Returns `None` at end of input.
pub(crate) async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>, DriverError> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().await.map_err(|error| {
            DriverError::new(
                ErrorKind::Io,
                format!("cannot read from connector: {error}"),
            )
        })?;
        if available.is_empty() {
            return Ok((!line.is_empty()).then_some(line));
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.unwrap_or(available.len());
        line.extend_from_slice(&available[..take]);
        reader.consume(newline.map_or(take, |position| position + 1));
        if line.len() > max_bytes {
            return Err(DriverError::new(
                ErrorKind::Protocol,
                format!("connector response exceeds {max_bytes} bytes"),
            ));
        }
        if newline.is_some() {
            return Ok(Some(line));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn splits_lines_and_enforces_limit() {
        let mut reader = BufReader::with_capacity(4, &b"first\nsecond"[..]);
        assert_eq!(
            read_bounded_line(&mut reader, 64).await.unwrap(),
            Some(b"first".to_vec())
        );
        assert_eq!(
            read_bounded_line(&mut reader, 64).await.unwrap(),
            Some(b"second".to_vec())
        );
        assert_eq!(read_bounded_line(&mut reader, 64).await.unwrap(), None);
        let mut long = BufReader::new(&b"0123456789\n"[..]);
        assert_eq!(
            read_bounded_line(&mut long, 4).await.unwrap_err().kind,
            ErrorKind::Protocol
        );
    }
}
