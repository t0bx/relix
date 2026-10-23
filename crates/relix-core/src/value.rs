/*
 * @author t0bx
 * @since 23.10.2026
 */

use serde::{Deserialize, Serialize};
use std::fmt::Write;

const BINARY_PREVIEW_BYTES: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Int(_) | Self::UInt(_) | Self::Float(_))
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    /// Full, lossless textual representation used for editing and exporting.
    pub fn to_plain_string(&self) -> Option<String> {
        match self {
            Self::Null => None,
            Self::Bool(value) => Some(value.to_string()),
            Self::Int(value) => Some(value.to_string()),
            Self::UInt(value) => Some(value.to_string()),
            Self::Float(value) => Some(value.to_string()),
            Self::Text(value) => Some(value.clone()),
            Self::Bytes(bytes) => Some(hex_string(bytes)),
        }
    }

    /// Compact single-line representation for grid cells.
    pub fn display_preview(&self, max_chars: usize) -> String {
        let raw = match self {
            Self::Null => return "NULL".to_owned(),
            Self::Bytes(bytes) => binary_preview(bytes),
            other => other.to_plain_string().unwrap_or_default(),
        };
        let single_line: String = raw
            .chars()
            .map(|ch| if ch == '\n' || ch == '\r' || ch == '\t' { ' ' } else { ch })
            .collect();
        truncate_chars(&single_line, max_chars)
    }

    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Bool(value) => serde_json::Value::Bool(*value),
            Self::Int(value) => serde_json::Value::from(*value),
            Self::UInt(value) => serde_json::Value::from(*value),
            Self::Float(value) => serde_json::Number::from_f64(*value)
                .map(serde_json::Value::Number)
                .unwrap_or_else(|| serde_json::Value::String(value.to_string())),
            Self::Text(value) => serde_json::Value::String(value.clone()),
            Self::Bytes(bytes) => serde_json::Value::String(hex_string(bytes)),
        }
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Null, Into::into)
    }
}

pub fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(2 + bytes.len() * 2);
    out.push_str("0x");
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn binary_preview(bytes: &[u8]) -> String {
    if bytes.len() <= BINARY_PREVIEW_BYTES {
        return hex_string(bytes);
    }
    format!(
        "{}… ({} bytes)",
        hex_string(&bytes[..BINARY_PREVIEW_BYTES]),
        bytes.len()
    )
}

pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut truncated: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_has_no_plain_string() {
        assert_eq!(Value::Null.to_plain_string(), None);
        assert_eq!(Value::Null.display_preview(10), "NULL");
    }

    #[test]
    fn previews_are_single_line_and_truncated() {
        let value = Value::Text("hello\nworld, this is long".into());
        assert_eq!(value.display_preview(11), "hello worl…");
    }

    #[test]
    fn binary_values_render_as_hex() {
        assert_eq!(Value::Bytes(vec![0xde, 0xad]).display_preview(20), "0xdead");
        let long = Value::Bytes(vec![0u8; 40]);
        assert!(long.display_preview(200).ends_with("(40 bytes)"));
    }

    #[test]
    fn json_conversion_keeps_types() {
        assert_eq!(Value::Int(4).to_json(), serde_json::json!(4));
        assert_eq!(Value::Null.to_json(), serde_json::Value::Null);
        assert_eq!(Value::Float(f64::NAN).to_json(), serde_json::json!("NaN"));
    }
}
