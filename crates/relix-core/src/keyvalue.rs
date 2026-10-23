/*
 * @author t0bx
 * @since 23.10.2026
 */

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct KeyName(pub Vec<u8>);

impl KeyName {
    pub fn display(&self) -> String {
        String::from_utf8_lossy(&self.0).into_owned()
    }
}

impl From<&str> for KeyName {
    fn from(value: &str) -> Self {
        Self(value.as_bytes().to_vec())
    }
}

impl fmt::Display for KeyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyType {
    String,
    List,
    Set,
    SortedSet,
    Hash,
    Stream,
    Other(String),
    Missing,
}

impl KeyType {
    pub fn from_redis_name(name: &str) -> Self {
        match name {
            "string" => Self::String,
            "list" => Self::List,
            "set" => Self::Set,
            "zset" => Self::SortedSet,
            "hash" => Self::Hash,
            "stream" => Self::Stream,
            "none" => Self::Missing,
            other => Self::Other(other.to_owned()),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::String => "string",
            Self::List => "list",
            Self::Set => "set",
            Self::SortedSet => "zset",
            Self::Hash => "hash",
            Self::Stream => "stream",
            Self::Other(name) => name,
            Self::Missing => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ttl {
    Persistent,
    Seconds(i64),
    Missing,
}

impl Ttl {
    pub fn from_redis(ttl: i64) -> Self {
        match ttl {
            -1 => Self::Persistent,
            -2 => Self::Missing,
            seconds => Self::Seconds(seconds),
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Persistent => "no expiry".to_owned(),
            Self::Missing => "missing".to_owned(),
            Self::Seconds(seconds) => format!("{seconds}s"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyScanPage {
    pub next_cursor: u64,
    pub keys: Vec<KeyName>,
}

impl KeyScanPage {
    pub fn is_complete(&self) -> bool {
        self.next_cursor == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyDescription {
    pub key: KeyName,
    pub key_type: KeyType,
    pub ttl: Ttl,
    pub length: Option<u64>,
    pub memory_bytes: Option<u64>,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueRange {
    pub cursor: u64,
    pub count: u32,
}

impl ValueRange {
    pub fn first(count: u32) -> Self {
        Self { cursor: 0, count }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamEntry {
    pub id: String,
    pub fields: Vec<(Vec<u8>, Vec<u8>)>,
}

/// One page of a key's content. `next_cursor` is `None` once the value is exhausted;
/// for lists and sorted sets it is an index offset, for hashes and sets a SCAN cursor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ValuePage {
    String {
        bytes: Vec<u8>,
        total_length: u64,
    },
    List {
        items: Vec<Vec<u8>>,
        start: u64,
        next_cursor: Option<u64>,
    },
    Set {
        members: Vec<Vec<u8>>,
        next_cursor: Option<u64>,
    },
    SortedSet {
        entries: Vec<(Vec<u8>, f64)>,
        start: u64,
        next_cursor: Option<u64>,
    },
    Hash {
        entries: Vec<(Vec<u8>, Vec<u8>)>,
        next_cursor: Option<u64>,
    },
    Stream {
        entries: Vec<StreamEntry>,
        next_cursor: Option<u64>,
    },
    Unsupported {
        key_type: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum KeyValueWrite {
    SetString {
        key: KeyName,
        value: Vec<u8>,
        ttl_seconds: Option<u64>,
    },
    HashSet {
        key: KeyName,
        field: Vec<u8>,
        value: Vec<u8>,
    },
    HashDelete {
        key: KeyName,
        field: Vec<u8>,
    },
    ListPush {
        key: KeyName,
        value: Vec<u8>,
        tail: bool,
    },
    ListSet {
        key: KeyName,
        index: i64,
        value: Vec<u8>,
    },
    SetAdd {
        key: KeyName,
        member: Vec<u8>,
    },
    SetRemove {
        key: KeyName,
        member: Vec<u8>,
    },
    SortedSetAdd {
        key: KeyName,
        member: Vec<u8>,
        score: f64,
    },
    SortedSetRemove {
        key: KeyName,
        member: Vec<u8>,
    },
    Expire {
        key: KeyName,
        seconds: u64,
    },
    Persist {
        key: KeyName,
    },
    Rename {
        key: KeyName,
        new_key: KeyName,
    },
    Delete {
        key: KeyName,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CommandReply {
    Nil,
    Integer(i64),
    Double(f64),
    Boolean(bool),
    Status(String),
    Bulk(Vec<u8>),
    Array(Vec<CommandReply>),
    Map(Vec<(CommandReply, CommandReply)>),
    Error(String),
}

impl CommandReply {
    /// Renders the reply like `redis-cli` does, with numbered nested entries.
    pub fn render(&self) -> Vec<String> {
        let mut lines = Vec::new();
        self.render_into(&mut lines, 0);
        lines
    }

    fn render_into(&self, lines: &mut Vec<String>, depth: usize) {
        let indent = "   ".repeat(depth);
        match self {
            Self::Array(items) if items.is_empty() => lines.push(format!("{indent}(empty array)")),
            Self::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.render_nested(lines, depth, index + 1, item);
                }
            }
            Self::Map(entries) if entries.is_empty() => lines.push(format!("{indent}(empty map)")),
            Self::Map(entries) => {
                for (index, (key, value)) in entries.iter().enumerate() {
                    self.render_nested(lines, depth, index * 2 + 1, key);
                    self.render_nested(lines, depth, index * 2 + 2, value);
                }
            }
            scalar => lines.push(format!("{indent}{}", scalar.scalar_text())),
        }
    }

    fn render_nested(&self, lines: &mut Vec<String>, depth: usize, number: usize, item: &Self) {
        let indent = "   ".repeat(depth);
        match item {
            Self::Array(_) | Self::Map(_) => {
                lines.push(format!("{indent}{number})"));
                item.render_into(lines, depth + 1);
            }
            scalar => lines.push(format!("{indent}{number}) {}", scalar.scalar_text())),
        }
    }

    fn scalar_text(&self) -> String {
        match self {
            Self::Nil => "(nil)".to_owned(),
            Self::Integer(value) => format!("(integer) {value}"),
            Self::Double(value) => format!("(double) {value}"),
            Self::Boolean(value) => format!("({value})"),
            Self::Status(status) => status.clone(),
            Self::Bulk(bytes) => format!("\"{}\"", String::from_utf8_lossy(bytes)),
            Self::Error(message) => format!("(error) {message}"),
            Self::Array(_) | Self::Map(_) => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttl_maps_redis_sentinels() {
        assert_eq!(Ttl::from_redis(-1), Ttl::Persistent);
        assert_eq!(Ttl::from_redis(-2), Ttl::Missing);
        assert_eq!(Ttl::from_redis(30), Ttl::Seconds(30));
    }

    #[test]
    fn replies_render_like_redis_cli() {
        let reply = CommandReply::Array(vec![
            CommandReply::Bulk(b"a".to_vec()),
            CommandReply::Array(vec![CommandReply::Integer(1)]),
        ]);
        assert_eq!(reply.render(), vec!["1) \"a\"", "2)", "   1) (integer) 1"]);
        assert_eq!(CommandReply::Nil.render(), vec!["(nil)"]);
    }
}
