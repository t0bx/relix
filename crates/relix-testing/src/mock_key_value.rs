/*
 * @author t0bx
 * @since 23.10.2026
 */

use async_trait::async_trait;
use relix_core::ConnectionConfig;
use relix_core::keyvalue::{
    CommandReply, KeyDescription, KeyName, KeyScanPage, KeyType, KeyValueWrite, Ttl, ValuePage,
    ValueRange,
};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata, KeyValueConnection, ServerInfo,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Debug, Clone, PartialEq)]
pub enum MockValue {
    String(Vec<u8>),
    List(Vec<Vec<u8>>),
    Hash(BTreeMap<Vec<u8>, Vec<u8>>),
}

type Store = Arc<Mutex<BTreeMap<Vec<u8>, MockValue>>>;

/// An in-memory key-value driver with Redis-like semantics for UI and workflow tests.
#[derive(Debug, Clone, Default)]
pub struct MockKeyValueDriver {
    store: Store,
}

impl MockKeyValueDriver {
    pub fn with_keys(keys: impl IntoIterator<Item = (&'static str, MockValue)>) -> Self {
        let store = keys
            .into_iter()
            .map(|(key, value)| (key.as_bytes().to_vec(), value))
            .collect();
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }

    pub fn snapshot(&self) -> BTreeMap<Vec<u8>, MockValue> {
        self.store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl DatabaseDriver for MockKeyValueDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            "mock-kv",
            "Mock KV",
            None,
            ConnectionTarget::Network { default_port: 1 },
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities::key_value()
    }

    async fn connect(
        &self,
        _config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        Ok(Box::new(MockKeyValueConnection {
            store: self.store.clone(),
        }))
    }
}

struct MockKeyValueConnection {
    store: Store,
}

impl MockKeyValueConnection {
    fn with<T>(&self, action: impl FnOnce(&mut BTreeMap<Vec<u8>, MockValue>) -> T) -> T {
        action(&mut self.store.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

#[async_trait]
impl DatabaseConnection for MockKeyValueConnection {
    fn server_info(&self) -> ServerInfo {
        ServerInfo {
            product: "MockKV".into(),
            version: "1".into(),
        }
    }

    async fn ping(&self) -> Result<(), DriverError> {
        Ok(())
    }

    async fn close(&self) -> Result<(), DriverError> {
        Ok(())
    }

    fn as_key_value(&self) -> Option<&dyn KeyValueConnection> {
        Some(self)
    }
}

fn matches_pattern(pattern: &str, key: &[u8]) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => key.starts_with(prefix.as_bytes()),
        None => key == pattern.as_bytes(),
    }
}

#[async_trait]
impl KeyValueConnection for MockKeyValueConnection {
    async fn scan_keys(
        &self,
        cursor: u64,
        pattern: &str,
        count: u32,
    ) -> Result<KeyScanPage, DriverError> {
        self.with(|store| {
            let keys: Vec<&Vec<u8>> = store
                .keys()
                .filter(|key| matches_pattern(pattern, key))
                .collect();
            let start = usize::try_from(cursor)
                .unwrap_or(usize::MAX)
                .min(keys.len());
            let end = (start + count as usize).min(keys.len());
            let next_cursor = if end >= keys.len() { 0 } else { end as u64 };
            Ok(KeyScanPage {
                next_cursor,
                keys: keys[start..end]
                    .iter()
                    .map(|key| KeyName((*key).clone()))
                    .collect(),
            })
        })
    }

    async fn describe_key(&self, key: &KeyName) -> Result<KeyDescription, DriverError> {
        self.with(|store| {
            let (key_type, length) = match store.get(&key.0) {
                Some(MockValue::String(bytes)) => (KeyType::String, bytes.len()),
                Some(MockValue::List(items)) => (KeyType::List, items.len()),
                Some(MockValue::Hash(entries)) => (KeyType::Hash, entries.len()),
                None => (KeyType::Missing, 0),
            };
            Ok(KeyDescription {
                key: key.clone(),
                key_type,
                ttl: Ttl::Persistent,
                length: Some(length as u64),
                memory_bytes: None,
                encoding: None,
            })
        })
    }

    async fn read_value(
        &self,
        key: &KeyName,
        _key_type: &KeyType,
        range: ValueRange,
    ) -> Result<ValuePage, DriverError> {
        self.with(|store| match store.get(&key.0) {
            Some(MockValue::String(bytes)) => Ok(ValuePage::String {
                bytes: bytes.iter().copied().take(range.count as usize).collect(),
                total_length: bytes.len() as u64,
            }),
            Some(MockValue::List(items)) => {
                let start = range.cursor as usize;
                let end = (start + range.count as usize).min(items.len());
                Ok(ValuePage::List {
                    items: items[start.min(end)..end].to_vec(),
                    start: range.cursor,
                    next_cursor: (end < items.len()).then_some(end as u64),
                })
            }
            Some(MockValue::Hash(entries)) => Ok(ValuePage::Hash {
                entries: entries
                    .iter()
                    .map(|(field, value)| (field.clone(), value.clone()))
                    .collect(),
                next_cursor: None,
            }),
            None => Ok(ValuePage::Unsupported {
                key_type: "none".into(),
            }),
        })
    }

    async fn write(&self, operation: &KeyValueWrite) -> Result<(), DriverError> {
        self.with(|store| {
            match operation.clone() {
                KeyValueWrite::SetString { key, value, .. } => {
                    store.insert(key.0, MockValue::String(value));
                }
                KeyValueWrite::HashSet { key, field, value } => match store
                    .entry(key.0)
                    .or_insert_with(|| MockValue::Hash(BTreeMap::new()))
                {
                    MockValue::Hash(entries) => {
                        entries.insert(field, value);
                    }
                    _ => return Err(DriverError::query("WRONGTYPE")),
                },
                KeyValueWrite::ListPush { key, value, .. } => match store
                    .entry(key.0)
                    .or_insert_with(|| MockValue::List(Vec::new()))
                {
                    MockValue::List(items) => items.push(value),
                    _ => return Err(DriverError::query("WRONGTYPE")),
                },
                KeyValueWrite::Delete { key } => {
                    store.remove(&key.0);
                }
                _ => {
                    return Err(DriverError::unsupported(
                        "operation not supported by the mock",
                    ));
                }
            }
            Ok(())
        })
    }

    async fn execute_command(&self, arguments: &[String]) -> Result<CommandReply, DriverError> {
        let upper: Vec<String> = arguments
            .iter()
            .map(|argument| argument.to_uppercase())
            .collect();
        match upper.first().map(String::as_str) {
            Some("PING") => Ok(CommandReply::Status("PONG".into())),
            Some("DBSIZE") => Ok(CommandReply::Integer(self.with(|store| store.len() as i64))),
            Some("GET") => Ok(self.with(|store| {
                match arguments.get(1).and_then(|key| store.get(key.as_bytes())) {
                    Some(MockValue::String(bytes)) => CommandReply::Bulk(bytes.clone()),
                    Some(_) => CommandReply::Error("WRONGTYPE".into()),
                    None => CommandReply::Nil,
                }
            })),
            Some(other) => Ok(CommandReply::Error(format!(
                "ERR unknown command '{other}'"
            ))),
            None => Err(DriverError::configuration("empty command")),
        }
    }

    async fn server_details(&self) -> Result<Vec<(String, String)>, DriverError> {
        Ok(vec![("server.version".into(), "1".into())])
    }
}
