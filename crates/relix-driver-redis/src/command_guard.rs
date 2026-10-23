/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::DriverError;

const BLOCKING_COMMANDS: [&str; 10] = [
    "BLPOP",
    "BRPOP",
    "BRPOPLPUSH",
    "BLMOVE",
    "BLMPOP",
    "BZPOPMIN",
    "BZPOPMAX",
    "BZMPOP",
    "WAIT",
    "WAITAOF",
];
const STREAMING_COMMANDS: [&str; 6] = [
    "SUBSCRIBE",
    "PSUBSCRIBE",
    "SSUBSCRIBE",
    "MONITOR",
    "SYNC",
    "PSYNC",
];
const SESSION_COMMANDS: [&str; 9] = [
    "SELECT", "RESET", "QUIT", "HELLO", "MULTI", "EXEC", "DISCARD", "WATCH", "UNWATCH",
];
const STREAM_READ_COMMANDS: [&str; 2] = ["XREAD", "XREADGROUP"];

/// Rejects commands that would block or take over the shared multiplexed connection
/// (blocking pops, subscriptions, replication streams) or silently change its session
/// state (database selection, transactions, protocol switches) for every other caller.
pub fn ensure_command_allowed(arguments: &[String]) -> Result<(), DriverError> {
    let Some(name) = arguments
        .first()
        .map(|name| name.trim().to_ascii_uppercase())
    else {
        return Err(DriverError::configuration("enter a Redis command"));
    };
    if name.is_empty() {
        return Err(DriverError::configuration("enter a Redis command"));
    }
    let options = &arguments[1..];
    if BLOCKING_COMMANDS.contains(&name.as_str()) || is_blocking_stream_read(&name, options) {
        return Err(DriverError::unsupported(format!(
            "{name} blocks the shared connection and is not supported in the console; \
             use the non-blocking variant instead"
        )));
    }
    if STREAMING_COMMANDS.contains(&name.as_str()) {
        return Err(DriverError::unsupported(format!(
            "{name} turns the connection into a push stream and is not supported in the console"
        )));
    }
    if SESSION_COMMANDS.contains(&name.as_str()) || is_client_reply(&name, options) {
        return Err(DriverError::unsupported(format!(
            "{name} changes the state of the shared connection and is not supported in the \
             console; switch databases through the data source settings"
        )));
    }
    Ok(())
}

fn is_blocking_stream_read(name: &str, options: &[String]) -> bool {
    STREAM_READ_COMMANDS.contains(&name)
        && options
            .iter()
            .take_while(|option| !option.eq_ignore_ascii_case("STREAMS"))
            .any(|option| option.eq_ignore_ascii_case("BLOCK"))
}

fn is_client_reply(name: &str, options: &[String]) -> bool {
    name == "CLIENT"
        && options
            .first()
            .is_some_and(|subcommand| subcommand.eq_ignore_ascii_case("REPLY"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_driver_api::ErrorKind;

    fn check(command: &str) -> Result<(), ErrorKind> {
        let arguments: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
        ensure_command_allowed(&arguments).map_err(|error| error.kind)
    }

    #[test]
    fn empty_commands_are_configuration_errors() {
        assert_eq!(check(""), Err(ErrorKind::Configuration));
        assert_eq!(
            ensure_command_allowed(&[" ".to_owned()]).map_err(|error| error.kind),
            Err(ErrorKind::Configuration)
        );
    }

    #[test]
    fn regular_commands_are_allowed() {
        assert_eq!(check("GET key"), Ok(()));
        assert_eq!(check("xread COUNT 5 STREAMS block 0"), Ok(()));
        assert_eq!(check("CLIENT LIST"), Ok(()));
    }

    #[test]
    fn blocking_and_hijacking_commands_are_rejected() {
        for command in [
            "blpop list 0",
            "BRPOP list 0",
            "BLMOVE a b LEFT RIGHT 0",
            "BZPOPMIN z 0",
            "BZPOPMAX z 0",
            "WAIT 1 0",
            "XREAD BLOCK 0 STREAMS s $",
            "XREADGROUP GROUP g c BLOCK 10 STREAMS s >",
            "SUBSCRIBE news",
            "psubscribe n*",
            "SSUBSCRIBE shard",
            "MONITOR",
            "SYNC",
            "PSYNC ? -1",
            "SELECT 2",
            "MULTI",
            "CLIENT REPLY OFF",
        ] {
            assert_eq!(check(command), Err(ErrorKind::Unsupported), "{command}");
        }
    }
}
