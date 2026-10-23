/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::runner::CommandRunner;
use redis::{Cmd, Value};
use relix_core::keyvalue::KeyValueWrite;
use relix_driver_api::DriverError;

pub async fn apply_write(
    runner: &CommandRunner,
    operation: &KeyValueWrite,
) -> Result<(), DriverError> {
    let reply: Value = runner.query(&write_command(operation)).await?;
    match (operation, reply) {
        (KeyValueWrite::Expire { key, .. }, Value::Int(0)) => Err(DriverError::query(format!(
            "cannot set expiry: key \"{key}\" does not exist"
        ))),
        _ => Ok(()),
    }
}

pub fn write_command(operation: &KeyValueWrite) -> Cmd {
    match operation {
        KeyValueWrite::SetString {
            key,
            value,
            ttl_seconds,
        } => {
            let mut command = command_for("SET", key.0.as_slice());
            command.arg(value.as_slice());
            if let Some(seconds) = ttl_seconds {
                command.arg("EX").arg(*seconds);
            }
            command
        }
        KeyValueWrite::HashSet { key, field, value } => {
            with_args(command_for("HSET", &key.0), &[field, value])
        }
        KeyValueWrite::HashDelete { key, field } => {
            with_args(command_for("HDEL", &key.0), &[field])
        }
        KeyValueWrite::ListPush { key, value, tail } => {
            let name = if *tail { "RPUSH" } else { "LPUSH" };
            with_args(command_for(name, &key.0), &[value])
        }
        KeyValueWrite::ListSet { key, index, value } => {
            let mut command = command_for("LSET", &key.0);
            command.arg(*index).arg(value.as_slice());
            command
        }
        KeyValueWrite::SetAdd { key, member } => with_args(command_for("SADD", &key.0), &[member]),
        KeyValueWrite::SetRemove { key, member } => {
            with_args(command_for("SREM", &key.0), &[member])
        }
        KeyValueWrite::SortedSetAdd { key, member, score } => {
            let mut command = command_for("ZADD", &key.0);
            command.arg(*score).arg(member.as_slice());
            command
        }
        KeyValueWrite::SortedSetRemove { key, member } => {
            with_args(command_for("ZREM", &key.0), &[member])
        }
        KeyValueWrite::Expire { key, seconds } => {
            let mut command = command_for("EXPIRE", &key.0);
            command.arg(*seconds);
            command
        }
        KeyValueWrite::Persist { key } => command_for("PERSIST", &key.0),
        KeyValueWrite::Rename { key, new_key } => {
            with_args(command_for("RENAME", &key.0), &[&new_key.0])
        }
        KeyValueWrite::Delete { key } => command_for("DEL", &key.0),
    }
}

fn command_for(name: &str, key: &[u8]) -> Cmd {
    let mut command = redis::cmd(name);
    command.arg(key);
    command
}

fn with_args(mut command: Cmd, arguments: &[&Vec<u8>]) -> Cmd {
    for argument in arguments {
        command.arg(argument.as_slice());
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::keyvalue::KeyName;

    fn rendered(operation: &KeyValueWrite) -> Vec<String> {
        write_command(operation)
            .args_iter()
            .filter_map(|argument| match argument {
                redis::Arg::Simple(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn set_string_appends_expiry_only_when_requested() {
        let key = KeyName::from("k");
        let with_ttl = KeyValueWrite::SetString {
            key: key.clone(),
            value: b"v".to_vec(),
            ttl_seconds: Some(60),
        };
        assert_eq!(rendered(&with_ttl), ["SET", "k", "v", "EX", "60"]);
        let without_ttl = KeyValueWrite::SetString {
            key,
            value: b"v".to_vec(),
            ttl_seconds: None,
        };
        assert_eq!(rendered(&without_ttl), ["SET", "k", "v"]);
    }

    #[test]
    fn list_push_picks_the_side() {
        let push = |tail| KeyValueWrite::ListPush {
            key: KeyName::from("l"),
            value: b"x".to_vec(),
            tail,
        };
        assert_eq!(rendered(&push(true)), ["RPUSH", "l", "x"]);
        assert_eq!(rendered(&push(false)), ["LPUSH", "l", "x"]);
    }

    #[test]
    fn sorted_set_add_places_score_before_member() {
        let operation = KeyValueWrite::SortedSetAdd {
            key: KeyName::from("z"),
            member: b"m".to_vec(),
            score: 1.5,
        };
        assert_eq!(rendered(&operation), ["ZADD", "z", "1.5", "m"]);
    }

    #[test]
    fn rename_passes_both_keys() {
        let operation = KeyValueWrite::Rename {
            key: KeyName::from("a"),
            new_key: KeyName::from("b"),
        };
        assert_eq!(rendered(&operation), ["RENAME", "a", "b"]);
    }
}
