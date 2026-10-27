/*
 * @author t0bx
 * @since 27.10.2026
 */

use crate::risk::Risk;

const READ_COMMANDS: &[&str] = &[
    "GET",
    "MGET",
    "GETRANGE",
    "SUBSTR",
    "STRLEN",
    "LCS",
    "EXISTS",
    "TYPE",
    "TTL",
    "PTTL",
    "EXPIRETIME",
    "PEXPIRETIME",
    "KEYS",
    "SCAN",
    "RANDOMKEY",
    "DBSIZE",
    "DUMP",
    "HGET",
    "HMGET",
    "HGETALL",
    "HKEYS",
    "HVALS",
    "HLEN",
    "HEXISTS",
    "HSCAN",
    "HSTRLEN",
    "HRANDFIELD",
    "HTTL",
    "HPTTL",
    "LRANGE",
    "LLEN",
    "LINDEX",
    "LPOS",
    "SMEMBERS",
    "SISMEMBER",
    "SMISMEMBER",
    "SCARD",
    "SSCAN",
    "SRANDMEMBER",
    "SINTER",
    "SUNION",
    "SDIFF",
    "SINTERCARD",
    "ZRANGE",
    "ZRANGEBYSCORE",
    "ZRANGEBYLEX",
    "ZREVRANGE",
    "ZREVRANGEBYSCORE",
    "ZREVRANGEBYLEX",
    "ZSCORE",
    "ZMSCORE",
    "ZCARD",
    "ZCOUNT",
    "ZLEXCOUNT",
    "ZRANK",
    "ZREVRANK",
    "ZSCAN",
    "ZRANDMEMBER",
    "ZINTER",
    "ZUNION",
    "ZDIFF",
    "ZINTERCARD",
    "XRANGE",
    "XREVRANGE",
    "XLEN",
    "XREAD",
    "XINFO",
    "XPENDING",
    "GEOPOS",
    "GEODIST",
    "GEOHASH",
    "GEOSEARCH",
    "GEORADIUS_RO",
    "GEORADIUSBYMEMBER_RO",
    "BITCOUNT",
    "BITPOS",
    "BITFIELD_RO",
    "GETBIT",
    "PFCOUNT",
    "OBJECT",
    "MEMORY",
    "INFO",
    "PING",
    "ECHO",
    "TIME",
    "LASTSAVE",
    "ROLE",
    "COMMAND",
    "PUBSUB",
    "EVAL_RO",
    "EVALSHA_RO",
    "FCALL_RO",
    "SELECT",
    "AUTH",
    "HELLO",
    "QUIT",
    "RESET",
    "READONLY",
    "READWRITE",
    "MULTI",
    "EXEC",
    "DISCARD",
    "WATCH",
    "UNWATCH",
    "SUBSCRIBE",
    "PSUBSCRIBE",
    "SSUBSCRIBE",
    "UNSUBSCRIBE",
    "PUNSUBSCRIBE",
    "SUNSUBSCRIBE",
    "JSON.GET",
    "JSON.MGET",
    "JSON.TYPE",
    "JSON.STRLEN",
    "JSON.ARRLEN",
    "JSON.OBJKEYS",
    "JSON.OBJLEN",
    "FT.SEARCH",
    "FT.AGGREGATE",
    "FT.INFO",
    "FT._LIST",
];

const COMMANDS_WITH_SUBCOMMANDS: &[&str] = &[
    "CONFIG", "CLIENT", "SLOWLOG", "ACL", "CLUSTER", "SCRIPT", "FUNCTION", "MODULE", "LATENCY",
];

const READ_SUBCOMMANDS: &[&str] = &[
    "GET",
    "LIST",
    "INFO",
    "GETNAME",
    "ID",
    "NODES",
    "SLOTS",
    "SHARDS",
    "WHOAMI",
    "USERS",
    "CAT",
    "LEN",
    "EXISTS",
    "STATS",
    "DOCTOR",
    "HISTORY",
    "LATEST",
    "HELP",
    "DOCS",
    "MYID",
    "KEYSLOT",
    "COUNTKEYSINSLOT",
    "GETKEYSINSLOT",
    "DUMP",
    "GRAPH",
    "HISTOGRAM",
    "TRACKINGINFO",
];

const REMOVING_COMMANDS: &[&str] = &["FLUSHALL", "FLUSHDB", "DEL", "UNLINK", "SWAPDB", "SHUTDOWN"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandTraits {
    pub writes: bool,
    pub risk: Option<Risk>,
}

/// Classifies a key-value console command (Redis/Valkey syntax) by its command and, for
/// administrative commands, its subcommand. Anything not known to be read-only counts as a
/// write, so unknown module or scripting commands are treated conservatively.
pub fn classify_command(arguments: &[String]) -> CommandTraits {
    let command = arguments
        .first()
        .map(|command| command.to_ascii_uppercase())
        .unwrap_or_default();
    let subcommand = arguments
        .get(1)
        .map(|subcommand| subcommand.to_ascii_uppercase())
        .unwrap_or_default();
    if command.is_empty() {
        return CommandTraits {
            writes: false,
            risk: None,
        };
    }
    let reads = if COMMANDS_WITH_SUBCOMMANDS.contains(&command.as_str()) {
        READ_SUBCOMMANDS.contains(&subcommand.as_str())
    } else {
        READ_COMMANDS.contains(&command.as_str())
    };
    let removes = REMOVING_COMMANDS.contains(&command.as_str())
        || (COMMANDS_WITH_SUBCOMMANDS.contains(&command.as_str()) && subcommand == "FLUSH");
    CommandTraits {
        writes: !reads,
        risk: removes.then_some(Risk::DataRemoval),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(line: &str) -> CommandTraits {
        let arguments: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
        classify_command(&arguments)
    }

    #[test]
    fn reading_commands_do_not_write() {
        assert!(!classify("get user:1").writes);
        assert!(!classify("HGETALL user:1").writes);
        assert!(!classify("scan 0 MATCH user:*").writes);
        assert!(!classify("CONFIG GET maxmemory").writes);
        assert!(!classify("client list").writes);
    }

    #[test]
    fn modifying_commands_write() {
        assert!(classify("SET a 1").writes);
        assert!(classify("CONFIG SET maxmemory 1gb").writes);
        assert!(classify("CLIENT KILL ID 4").writes);
        assert!(classify("EVAL \"return 1\" 0").writes);
        assert!(classify("SOMEMODULE.CMD x").writes);
    }

    #[test]
    fn data_removal_is_flagged() {
        assert_eq!(classify("FLUSHALL").risk, Some(Risk::DataRemoval));
        assert_eq!(classify("del a b").risk, Some(Risk::DataRemoval));
        assert_eq!(classify("SCRIPT FLUSH").risk, Some(Risk::DataRemoval));
        assert_eq!(classify("SET a 1").risk, None);
    }

    #[test]
    fn empty_commands_are_harmless() {
        assert!(!classify_command(&[]).writes);
    }
}
