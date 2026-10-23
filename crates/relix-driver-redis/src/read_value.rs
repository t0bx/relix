/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::protocol_error;
use crate::runner::CommandRunner;
use relix_core::keyvalue::{KeyName, KeyType, StreamEntry, ValuePage, ValueRange};
use relix_driver_api::DriverError;

type Pair = (Vec<u8>, Vec<u8>);

/// Reads one bounded page of a key's value so huge values never load at once. Lists and
/// sorted sets page by index (fetching one extra element to detect more data), sets and
/// hashes by SCAN cursor, strings by byte prefix (`range.count` bytes). Streams only
/// return their first `range.count` entries because a stream ID does not fit the numeric
/// cursor.
pub async fn read_value(
    runner: &CommandRunner,
    key: &KeyName,
    key_type: &KeyType,
    range: ValueRange,
) -> Result<ValuePage, DriverError> {
    match key_type {
        KeyType::String => read_string(runner, key, range.count).await,
        KeyType::List => read_list(runner, key, range).await,
        KeyType::Set => read_set(runner, key, range).await,
        KeyType::SortedSet => read_sorted_set(runner, key, range).await,
        KeyType::Hash => read_hash(runner, key, range).await,
        KeyType::Stream => read_stream(runner, key, range.count).await,
        KeyType::Other(_) | KeyType::Missing => Ok(ValuePage::Unsupported {
            key_type: key_type.label().to_owned(),
        }),
    }
}

async fn read_string(
    runner: &CommandRunner,
    key: &KeyName,
    max_bytes: u32,
) -> Result<ValuePage, DriverError> {
    if max_bytes == 0 {
        let total_length = runner.query(redis::cmd("STRLEN").arg(&key.0)).await?;
        return Ok(ValuePage::String {
            bytes: Vec::new(),
            total_length,
        });
    }
    let mut pipeline = redis::pipe();
    pipeline
        .cmd("GETRANGE")
        .arg(&key.0)
        .arg(0)
        .arg(i64::from(max_bytes) - 1)
        .cmd("STRLEN")
        .arg(&key.0);
    let (bytes, total_length): (Vec<u8>, u64) = runner.query_pipeline(&pipeline).await?;
    Ok(ValuePage::String {
        bytes,
        total_length,
    })
}

async fn read_list(
    runner: &CommandRunner,
    key: &KeyName,
    range: ValueRange,
) -> Result<ValuePage, DriverError> {
    let window = IndexWindow::new(range);
    let mut command = redis::cmd("LRANGE");
    command
        .arg(&key.0)
        .arg(window.start)
        .arg(window.probe_stop());
    let items: Vec<Vec<u8>> = runner.query(&command).await?;
    let (items, next_cursor) = window.split(items);
    Ok(ValuePage::List {
        items,
        start: range.cursor,
        next_cursor,
    })
}

async fn read_sorted_set(
    runner: &CommandRunner,
    key: &KeyName,
    range: ValueRange,
) -> Result<ValuePage, DriverError> {
    let window = IndexWindow::new(range);
    let mut command = redis::cmd("ZRANGE");
    command
        .arg(&key.0)
        .arg(window.start)
        .arg(window.probe_stop())
        .arg("WITHSCORES");
    let flat: Vec<Vec<u8>> = runner.query(&command).await?;
    let entries = pairs(flat)?
        .into_iter()
        .map(|(member, score)| parse_score(&score).map(|score| (member, score)))
        .collect::<Result<Vec<_>, _>>()?;
    let (entries, next_cursor) = window.split(entries);
    Ok(ValuePage::SortedSet {
        entries,
        start: range.cursor,
        next_cursor,
    })
}

async fn read_set(
    runner: &CommandRunner,
    key: &KeyName,
    range: ValueRange,
) -> Result<ValuePage, DriverError> {
    let (cursor, members) = scan_value(runner, "SSCAN", key, range).await?;
    Ok(ValuePage::Set {
        members,
        next_cursor: scan_continuation(cursor),
    })
}

async fn read_hash(
    runner: &CommandRunner,
    key: &KeyName,
    range: ValueRange,
) -> Result<ValuePage, DriverError> {
    let (cursor, flat) = scan_value(runner, "HSCAN", key, range).await?;
    Ok(ValuePage::Hash {
        entries: pairs(flat)?,
        next_cursor: scan_continuation(cursor),
    })
}

async fn read_stream(
    runner: &CommandRunner,
    key: &KeyName,
    count: u32,
) -> Result<ValuePage, DriverError> {
    let mut command = redis::cmd("XRANGE");
    command
        .arg(&key.0)
        .arg("-")
        .arg("+")
        .arg("COUNT")
        .arg(count.max(1));
    let raw: Vec<(String, Vec<Vec<u8>>)> = runner.query(&command).await?;
    let entries = raw
        .into_iter()
        .map(|(id, fields)| pairs(fields).map(|fields| StreamEntry { id, fields }))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ValuePage::Stream {
        entries,
        next_cursor: None,
    })
}

async fn scan_value(
    runner: &CommandRunner,
    command_name: &str,
    key: &KeyName,
    range: ValueRange,
) -> Result<(u64, Vec<Vec<u8>>), DriverError> {
    let mut command = redis::cmd(command_name);
    command
        .arg(&key.0)
        .arg(range.cursor)
        .arg("COUNT")
        .arg(range.count.max(1));
    runner.query(&command).await
}

fn scan_continuation(cursor: u64) -> Option<u64> {
    (cursor != 0).then_some(cursor)
}

struct IndexWindow {
    start: u64,
    count: u64,
}

impl IndexWindow {
    fn new(range: ValueRange) -> Self {
        Self {
            start: range.cursor,
            count: u64::from(range.count.max(1)),
        }
    }

    fn probe_stop(&self) -> u64 {
        self.start.saturating_add(self.count)
    }

    fn split<T>(&self, mut items: Vec<T>) -> (Vec<T>, Option<u64>) {
        let limit = usize::try_from(self.count).unwrap_or(usize::MAX);
        if items.len() > limit {
            items.truncate(limit);
            (items, Some(self.start.saturating_add(self.count)))
        } else {
            (items, None)
        }
    }
}

fn pairs(flat: Vec<Vec<u8>>) -> Result<Vec<Pair>, DriverError> {
    if !flat.len().is_multiple_of(2) {
        return Err(protocol_error(
            "expected an even number of field/value elements",
        ));
    }
    let mut items = flat.into_iter();
    let mut result = Vec::with_capacity(items.len() / 2);
    while let (Some(first), Some(second)) = (items.next(), items.next()) {
        result.push((first, second));
    }
    Ok(result)
}

fn parse_score(raw: &[u8]) -> Result<f64, DriverError> {
    std::str::from_utf8(raw)
        .ok()
        .and_then(|text| text.parse::<f64>().ok())
        .ok_or_else(|| {
            protocol_error(format!(
                "invalid sorted set score {}",
                String::from_utf8_lossy(raw)
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_window_requests_one_extra_element() {
        let window = IndexWindow::new(ValueRange {
            cursor: 10,
            count: 5,
        });
        assert_eq!(window.probe_stop(), 15);
        assert_eq!(
            window.split(vec![1, 2, 3, 4, 5, 6]),
            (vec![1, 2, 3, 4, 5], Some(15))
        );
        assert_eq!(window.split(vec![1, 2]), (vec![1, 2], None));
    }

    #[test]
    fn zero_count_still_reads_one_element() {
        let window = IndexWindow::new(ValueRange::first(0));
        assert_eq!(window.probe_stop(), 1);
    }

    #[test]
    fn scan_cursor_zero_ends_the_value() {
        assert_eq!(scan_continuation(0), None);
        assert_eq!(scan_continuation(17), Some(17));
    }

    #[test]
    fn flat_replies_pair_up() {
        let flat = vec![b"a".to_vec(), b"1".to_vec(), b"b".to_vec(), b"2".to_vec()];
        assert_eq!(
            pairs(flat).unwrap(),
            vec![
                (b"a".to_vec(), b"1".to_vec()),
                (b"b".to_vec(), b"2".to_vec())
            ]
        );
        assert!(pairs(vec![b"odd".to_vec()]).is_err());
    }

    #[test]
    fn scores_parse_including_infinity() {
        assert_eq!(parse_score(b"1.5").unwrap(), 1.5);
        assert_eq!(parse_score(b"-inf").unwrap(), f64::NEG_INFINITY);
        assert!(parse_score(b"abc").is_err());
    }
}
