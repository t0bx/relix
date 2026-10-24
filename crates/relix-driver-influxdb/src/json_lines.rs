/*
 * @author t0bx
 * @since 24.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::{Response, map_transport_error};
use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::Value as Json;
use std::fmt;

/// One JSON Lines record with its keys in the order the server wrote them, which is the
/// projection order. serde_json's default map sorts keys, so records are read into a
/// list of pairs instead.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Record(pub(crate) Vec<(String, Json)>);

impl Record {
    pub(crate) fn get(&self, key: &str) -> Option<&Json> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }
}

impl<'de> Deserialize<'de> for Record {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RecordVisitor;

        impl<'de> Visitor<'de> for RecordVisitor {
            type Value = Record;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Record, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, Json>()? {
                    entries.push(entry);
                }
                Ok(Record(entries))
            }
        }

        deserializer.deserialize_map(RecordVisitor)
    }
}

/// Reads records from a streamed JSON Lines response until `limit` records were
/// collected. When one more record arrives the response is dropped, which closes the
/// stream instead of transferring the rest of the result, and `has_more` is set.
pub(crate) async fn read_records(
    mut response: Response,
    limit: usize,
) -> Result<(Vec<Record>, bool), DriverError> {
    let mut records = Vec::new();
    let mut buffer: Vec<u8> = Vec::new();
    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|error| map_transport_error(&error))?;
        let finished = chunk.is_none();
        if let Some(chunk) = chunk {
            buffer.extend_from_slice(&chunk);
        } else if !buffer.is_empty() {
            buffer.push(b'\n');
        }
        while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            let Some(record) = parse_line(&line)? else {
                continue;
            };
            if records.len() == limit {
                return Ok((records, true));
            }
            records.push(record);
        }
        if finished {
            return Ok((records, false));
        }
    }
}

fn parse_line(line: &[u8]) -> Result<Option<Record>, DriverError> {
    let trimmed = line.trim_ascii();
    if trimmed.is_empty() {
        return Ok(None);
    }
    serde_json::from_slice(trimmed).map(Some).map_err(|error| {
        DriverError::new(
            ErrorKind::Protocol,
            "invalid JSON Lines record from the server",
        )
        .with_detail(error.to_string())
    })
}

/// Collects the column names of all records. Null values are omitted from a record, so
/// a key first seen in a later record is placed right after the key preceding it there.
pub(crate) fn column_order(records: &[Record]) -> Vec<String> {
    let mut columns: Vec<String> = Vec::new();
    for record in records {
        let mut previous: Option<usize> = None;
        for (key, _) in &record.0 {
            match columns.iter().position(|column| column == key) {
                Some(index) => previous = Some(index),
                None => {
                    let index = previous.map_or(0, |index| index + 1);
                    columns.insert(index, key.clone());
                    previous = Some(index);
                }
            }
        }
    }
    columns
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(line: &str) -> Record {
        parse_line(line.as_bytes()).unwrap().unwrap()
    }

    #[test]
    fn records_keep_the_server_key_order() {
        let parsed = record(r#"{"usage":1.5,"host":"a","time":"2024-01-01T00:00:00"}"#);
        let keys: Vec<&str> = parsed.0.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, ["usage", "host", "time"]);
        assert_eq!(parsed.get("host"), Some(&Json::from("a")));
        assert!(parse_line(b"  \n").unwrap().is_none());
        assert!(parse_line(b"{oops").is_err());
    }

    #[test]
    fn columns_missing_in_early_records_keep_their_position() {
        let records = vec![
            record(r#"{"usage":1,"time":"t"}"#),
            record(r#"{"usage":2,"region":"eu","time":"t","extra":true}"#),
            record(r#"{"first":0,"usage":3}"#),
        ];
        assert_eq!(
            column_order(&records),
            ["first", "usage", "region", "time", "extra"]
        );
    }
}
