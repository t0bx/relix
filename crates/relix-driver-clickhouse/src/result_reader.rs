/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::ServerException;
use crate::line_reader::LineReader;
use crate::values::from_json;
use relix_core::{ColumnMeta, RowBatch, Value};
use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::Response;
use serde_json::value::RawValue;

const SUMMARY_HEADER: &str = "X-ClickHouse-Summary";
const PLAIN_TEXT_COLUMN: &str = "result";

pub(crate) struct QueryOutcome {
    pub(crate) batch: Option<RowBatch>,
    pub(crate) written_rows: u64,
    pub(crate) truncated: bool,
}

enum Line {
    Row(Vec<Value>),
    Exception(Vec<Value>, ServerException),
}

struct RowCollector {
    rows: Vec<Vec<Value>>,
    limit: usize,
    has_more: bool,
}

impl RowCollector {
    fn push(&mut self, row: Vec<Value>) -> bool {
        if self.rows.len() == self.limit {
            self.has_more = true;
            return false;
        }
        self.rows.push(row);
        true
    }
}

/// Reads a `JSONCompactEachRowWithNamesAndTypes` body: column names, column types and
/// one JSON array per row, stopping after `limit` rows plus one look-ahead row. A query
/// that fails after streaming started ends the body with its exception as a one-cell
/// row; for single-column results that row is only treated as an error when nothing
/// follows it. Bodies in another format (an explicit `FORMAT` clause) are returned as
/// one text column per line. An empty body means the statement has no result set.
pub(crate) async fn read_result(
    response: Response,
    limit: usize,
    sql: &str,
) -> Result<QueryOutcome, DriverError> {
    let written_rows = written_rows(&response);
    let mut lines = LineReader::new(response);
    let Some(first) = lines.next_line().await? else {
        return Ok(QueryOutcome {
            batch: None,
            written_rows,
            truncated: false,
        });
    };
    let mut collector = RowCollector {
        rows: Vec::new(),
        limit,
        has_more: false,
    };
    let header = serde_json::from_str::<Vec<String>>(&first)
        .ok()
        .filter(|names| !names.is_empty());
    let columns = match header {
        Some(names) => {
            let types = read_types(&mut lines, names.len()).await?;
            read_json_rows(&mut lines, &types, &mut collector, sql).await?;
            names
                .into_iter()
                .zip(types)
                .map(|(name, type_name)| ColumnMeta::new(name, Some(type_name)))
                .collect()
        }
        None => {
            read_plain_rows(first, &mut lines, &mut collector, sql).await?;
            vec![ColumnMeta::new(PLAIN_TEXT_COLUMN, None)]
        }
    };
    Ok(QueryOutcome {
        truncated: collector.has_more,
        batch: Some(RowBatch {
            columns,
            rows: collector.rows,
            has_more: collector.has_more,
        }),
        written_rows,
    })
}

async fn read_types(lines: &mut LineReader, width: usize) -> Result<Vec<String>, DriverError> {
    let line = lines.next_line().await?.unwrap_or_default();
    match serde_json::from_str::<Vec<String>>(&line) {
        Ok(types) if types.len() == width => Ok(types),
        _ => Err(unexpected_line(&line)),
    }
}

async fn read_json_rows(
    lines: &mut LineReader,
    types: &[String],
    collector: &mut RowCollector,
    sql: &str,
) -> Result<(), DriverError> {
    let mut pending: Option<(Vec<Value>, ServerException)> = None;
    while let Some(line) = lines.next_line().await? {
        if line.is_empty() {
            continue;
        }
        if let Some((row, _)) = pending.take()
            && !collector.push(row)
        {
            return Ok(());
        }
        match parse_row(&line, types)? {
            Line::Exception(_, exception) if types.len() != 1 => {
                return Err(exception.into_error(None, Some(sql)));
            }
            Line::Exception(row, exception) => pending = Some((row, exception)),
            Line::Row(row) => {
                if !collector.push(row) {
                    return Ok(());
                }
            }
        }
    }
    match pending {
        Some((_, exception)) => Err(exception.into_error(None, Some(sql))),
        None => Ok(()),
    }
}

async fn read_plain_rows(
    first: String,
    lines: &mut LineReader,
    collector: &mut RowCollector,
    sql: &str,
) -> Result<(), DriverError> {
    let mut current = Some(first);
    while let Some(line) = current {
        let next = lines.next_line().await?;
        if next.is_none()
            && let Some(exception) = ServerException::from_row_text(&line)
        {
            return Err(exception.into_error(None, Some(sql)));
        }
        if !collector.push(vec![Value::Text(line)]) {
            return Ok(());
        }
        current = next;
    }
    Ok(())
}

fn parse_row(line: &str, types: &[String]) -> Result<Line, DriverError> {
    let Ok(cells) = serde_json::from_str::<Vec<&RawValue>>(line) else {
        return match ServerException::from_row_text(line) {
            Some(exception) => Ok(Line::Exception(Vec::new(), exception)),
            None => Err(unexpected_line(line)),
        };
    };
    let exception = match cells.as_slice() {
        [cell] => serde_json::from_str::<String>(cell.get())
            .ok()
            .and_then(|text| ServerException::from_row_text(&text)),
        _ => None,
    };
    if cells.len() != types.len() {
        return exception
            .map(|exception| Line::Exception(Vec::new(), exception))
            .ok_or_else(|| unexpected_line(line));
    }
    let row = cells
        .iter()
        .zip(types)
        .map(|(cell, type_name)| from_json(cell, type_name))
        .collect();
    Ok(match exception {
        Some(exception) => Line::Exception(row, exception),
        None => Line::Row(row),
    })
}

fn written_rows(response: &Response) -> u64 {
    response
        .headers()
        .get(SUMMARY_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|summary| serde_json::from_str::<serde_json::Value>(summary).ok())
        .and_then(|summary| {
            summary
                .get("written_rows")
                .and_then(serde_json::Value::as_str)
                .and_then(|rows| rows.parse().ok())
        })
        .unwrap_or_default()
}

fn unexpected_line(line: &str) -> DriverError {
    let excerpt: String = line.chars().take(200).collect();
    DriverError::new(ErrorKind::Protocol, "unexpected line in the result stream")
        .with_detail(excerpt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn rows_are_converted_by_column_type() {
        let Ok(Line::Row(row)) = parse_row(
            "[1, \"x\", null]",
            &types(&["UInt8", "String", "Nullable(Int64)"]),
        ) else {
            panic!("expected a row");
        };
        assert_eq!(
            row,
            vec![Value::Int(1), Value::Text("x".into()), Value::Null]
        );
    }

    #[test]
    fn wrapped_exceptions_are_detected() {
        let line = "[\"Code: 395. DB::Exception: Value passed to 'throwIf' function is non-zero. \
                    (FUNCTION_THROW_IF_VALUE_IS_NON_ZERO) (version 25.8)\"]";
        assert!(matches!(
            parse_row(line, &types(&["UInt64", "UInt8"])),
            Ok(Line::Exception(_, exception)) if exception.code == Some(395)
        ));
        assert!(matches!(
            parse_row(line, &types(&["String"])),
            Ok(Line::Exception(row, _)) if row.len() == 1
        ));
        let error = parse_row("garbage", &types(&["String"])).err().unwrap();
        assert_eq!(error.kind, ErrorKind::Protocol);
    }
}
