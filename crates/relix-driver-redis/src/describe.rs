/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::{map_parsing_error, map_redis_error};
use crate::runner::CommandRunner;
use redis::Value;
use relix_core::keyvalue::{KeyDescription, KeyName, KeyType, Ttl};
use relix_driver_api::DriverError;

/// Describes a key with one pipeline for type, TTL, encoding and memory usage, followed by
/// the type specific length command. `MEMORY USAGE` is best effort: servers that deny it
/// (ACLs, managed offerings) simply report no memory figure.
pub async fn describe_key(
    runner: &CommandRunner,
    key: &KeyName,
) -> Result<KeyDescription, DriverError> {
    let mut pipeline = redis::pipe();
    pipeline
        .cmd("TYPE")
        .arg(&key.0)
        .cmd("TTL")
        .arg(&key.0)
        .cmd("OBJECT")
        .arg("ENCODING")
        .arg(&key.0)
        .cmd("MEMORY")
        .arg("USAGE")
        .arg(&key.0);
    let mut replies = runner.raw_pipeline(&pipeline).await?.into_iter();
    let type_name: String = required(replies.next())?;
    let ttl: i64 = required(replies.next())?;
    let encoding = optional::<String>(replies.next());
    let memory_bytes = optional::<u64>(replies.next());
    let key_type = KeyType::from_redis_name(&type_name);
    let length = match length_command(&key_type) {
        Some(name) => Some(runner.query(redis::cmd(name).arg(&key.0)).await?),
        None => None,
    };
    Ok(KeyDescription {
        key: key.clone(),
        key_type,
        ttl: Ttl::from_redis(ttl),
        length,
        memory_bytes,
        encoding,
    })
}

pub fn length_command(key_type: &KeyType) -> Option<&'static str> {
    match key_type {
        KeyType::String => Some("STRLEN"),
        KeyType::List => Some("LLEN"),
        KeyType::Set => Some("SCARD"),
        KeyType::SortedSet => Some("ZCARD"),
        KeyType::Hash => Some("HLEN"),
        KeyType::Stream => Some("XLEN"),
        KeyType::Other(_) | KeyType::Missing => None,
    }
}

fn required<T: redis::FromRedisValue>(reply: Option<Value>) -> Result<T, DriverError> {
    let value = reply
        .ok_or_else(|| DriverError::internal("Redis returned fewer replies than requested"))?
        .extract_error()
        .map_err(map_redis_error)?;
    redis::from_redis_value(value).map_err(map_parsing_error)
}

fn optional<T: redis::FromRedisValue>(reply: Option<Value>) -> Option<T> {
    match reply? {
        Value::Nil | Value::ServerError(_) => None,
        value => redis::from_redis_value(value).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_type_has_a_length_command() {
        assert_eq!(length_command(&KeyType::String), Some("STRLEN"));
        assert_eq!(length_command(&KeyType::SortedSet), Some("ZCARD"));
        assert_eq!(length_command(&KeyType::Stream), Some("XLEN"));
        assert_eq!(length_command(&KeyType::Missing), None);
        assert_eq!(length_command(&KeyType::Other("ReJSON-RL".into())), None);
    }

    #[test]
    fn optional_replies_ignore_errors_and_nil() {
        let error = redis::parse_redis_value(b"-ERR denied\r\n").unwrap();
        assert_eq!(optional::<u64>(Some(error)), None);
        assert_eq!(optional::<u64>(Some(Value::Nil)), None);
        assert_eq!(optional::<u64>(Some(Value::Int(56))), Some(56));
    }
}
