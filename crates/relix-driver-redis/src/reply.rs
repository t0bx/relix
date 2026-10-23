/*
 * @author t0bx
 * @since 23.10.2026
 */

use redis::{ServerError, Value};
use relix_core::keyvalue::CommandReply;

pub fn command_reply(value: Value) -> CommandReply {
    match value {
        Value::Nil => CommandReply::Nil,
        Value::Int(number) => CommandReply::Integer(number),
        Value::BulkString(bytes) => CommandReply::Bulk(bytes),
        Value::Array(items) | Value::Set(items) => {
            CommandReply::Array(items.into_iter().map(command_reply).collect())
        }
        Value::SimpleString(status) => CommandReply::Status(status),
        Value::Okay => CommandReply::Status("OK".to_owned()),
        Value::Map(entries) => CommandReply::Map(
            entries
                .into_iter()
                .map(|(key, value)| (command_reply(key), command_reply(value)))
                .collect(),
        ),
        Value::Attribute { data, .. } => command_reply(*data),
        Value::Double(number) => CommandReply::Double(number),
        Value::Boolean(flag) => CommandReply::Boolean(flag),
        Value::VerbatimString { text, .. } => CommandReply::Bulk(text.into_bytes()),
        Value::BigNumber(number) => CommandReply::Status(number.to_string()),
        Value::Push { kind, data } => CommandReply::Array(
            std::iter::once(CommandReply::Status(kind.to_string()))
                .chain(data.into_iter().map(command_reply))
                .collect(),
        ),
        Value::ServerError(error) => CommandReply::Error(server_error_text(&error)),
        unknown => CommandReply::Status(format!("{unknown:?}")),
    }
}

fn server_error_text(error: &ServerError) -> String {
    match error.details() {
        Some(details) if !details.is_empty() => format!("{} {details}", error.code()),
        _ => error.code().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use redis::{PushKind, VerbatimFormat};

    fn parsed(resp: &[u8]) -> Value {
        redis::parse_redis_value(resp).unwrap()
    }

    #[test]
    fn scalars_convert_directly() {
        assert_eq!(command_reply(Value::Nil), CommandReply::Nil);
        assert_eq!(command_reply(Value::Int(7)), CommandReply::Integer(7));
        assert_eq!(
            command_reply(Value::BulkString(b"v".to_vec())),
            CommandReply::Bulk(b"v".to_vec())
        );
        assert_eq!(
            command_reply(Value::SimpleString("PONG".to_owned())),
            CommandReply::Status("PONG".to_owned())
        );
        assert_eq!(
            command_reply(Value::Okay),
            CommandReply::Status("OK".to_owned())
        );
        assert_eq!(command_reply(Value::Double(1.5)), CommandReply::Double(1.5));
        assert_eq!(
            command_reply(Value::Boolean(true)),
            CommandReply::Boolean(true)
        );
    }

    #[test]
    fn collections_convert_recursively() {
        let array = Value::Array(vec![Value::Int(1), Value::Array(vec![Value::Nil])]);
        assert_eq!(
            command_reply(array),
            CommandReply::Array(vec![
                CommandReply::Integer(1),
                CommandReply::Array(vec![CommandReply::Nil]),
            ])
        );
        assert_eq!(
            command_reply(Value::Set(vec![Value::Int(2)])),
            CommandReply::Array(vec![CommandReply::Integer(2)])
        );
        let map = Value::Map(vec![(Value::SimpleString("k".to_owned()), Value::Int(3))]);
        assert_eq!(
            command_reply(map),
            CommandReply::Map(vec![(
                CommandReply::Status("k".to_owned()),
                CommandReply::Integer(3)
            )])
        );
    }

    #[test]
    fn wrappers_unwrap_to_their_payload() {
        let attribute = Value::Attribute {
            data: Box::new(Value::Int(4)),
            attributes: vec![(Value::Nil, Value::Nil)],
        };
        assert_eq!(command_reply(attribute), CommandReply::Integer(4));
        let verbatim = Value::VerbatimString {
            format: VerbatimFormat::Text,
            text: "hello".to_owned(),
        };
        assert_eq!(
            command_reply(verbatim),
            CommandReply::Bulk(b"hello".to_vec())
        );
        let push = Value::Push {
            kind: PushKind::Message,
            data: vec![Value::BulkString(b"x".to_vec())],
        };
        assert_eq!(
            command_reply(push),
            CommandReply::Array(vec![
                CommandReply::Status("message".to_owned()),
                CommandReply::Bulk(b"x".to_vec()),
            ])
        );
    }

    #[test]
    fn big_numbers_render_as_text() {
        let value = parsed(b"(3492890328409238509324850943850943825024385\r\n");
        assert_eq!(
            command_reply(value),
            CommandReply::Status("3492890328409238509324850943850943825024385".to_owned())
        );
    }

    #[test]
    fn server_errors_become_error_replies() {
        let value = parsed(b"-ERR unknown command 'NOPE'\r\n");
        assert_eq!(
            command_reply(value),
            CommandReply::Error("ERR unknown command 'NOPE'".to_owned())
        );
    }
}
