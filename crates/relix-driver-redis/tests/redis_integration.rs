/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used, clippy::expect_used)]

use relix_core::keyvalue::{
    CommandReply, KeyName, KeyType, KeyValueWrite, Ttl, ValuePage, ValueRange,
};
use relix_core::{ConnectionConfig, Secret, TlsMode};
use relix_driver_api::{DatabaseConnection, DatabaseDriver, ErrorKind, KeyValueConnection};
use relix_driver_redis::RedisDriver;
use relix_testing::integration::{TestEngine, integration_config, unique_name};
use std::collections::BTreeSet;

async fn connect_with(config: &ConnectionConfig) -> Box<dyn DatabaseConnection> {
    RedisDriver::new().connect(config).await.expect("connect")
}

async fn connect() -> Option<Box<dyn DatabaseConnection>> {
    let config = integration_config(TestEngine::Redis)?;
    Some(connect_with(&config).await)
}

fn kv(connection: &dyn DatabaseConnection) -> &dyn KeyValueConnection {
    connection.as_key_value().expect("key-value connection")
}

fn arguments(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

async fn run(connection: &dyn KeyValueConnection, parts: &[&str]) -> CommandReply {
    connection
        .execute_command(&arguments(parts))
        .await
        .expect("command")
}

async fn delete_keys(connection: &dyn KeyValueConnection, keys: &[String]) {
    let mut command = vec!["DEL".to_owned()];
    command.extend(keys.iter().cloned());
    connection.execute_command(&command).await.expect("cleanup");
}

async fn delete_matching(connection: &dyn KeyValueConnection, pattern: &str) {
    let mut cursor = 0;
    let mut keys = Vec::new();
    loop {
        let page = connection.scan_keys(cursor, pattern, 500).await.unwrap();
        keys.extend(page.keys.iter().map(KeyName::display));
        if page.is_complete() {
            break;
        }
        cursor = page.next_cursor;
    }
    if !keys.is_empty() {
        delete_keys(connection, &keys).await;
    }
}

#[tokio::test]
async fn connects_pings_and_reports_server_info() {
    let Some(connection) = connect().await else {
        return;
    };
    let info = connection.server_info();
    assert_eq!(info.product, "Redis");
    assert!(
        info.version.starts_with(char::is_numeric),
        "{}",
        info.version
    );
    connection.ping().await.unwrap();
    assert!(connection.as_sql().is_none());
    connection.close().await.unwrap();
    let error = connection.ping().await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Connection);
}

#[tokio::test]
async fn wrong_password_is_an_authentication_error() {
    let Some(mut config) = integration_config(TestEngine::Redis) else {
        return;
    };
    config.password = Some(Secret::new("definitely-wrong"));
    let error = RedisDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication, "{error}");
    assert!(!error.to_string().contains("definitely-wrong"));
}

#[tokio::test]
async fn missing_password_is_an_authentication_error() {
    let Some(mut config) = integration_config(TestEngine::Redis) else {
        return;
    };
    config.password = None;
    let error = RedisDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Authentication, "{error}");
}

#[tokio::test]
async fn wrong_port_is_a_connection_error() {
    let Some(mut config) = integration_config(TestEngine::Redis) else {
        return;
    };
    config.settings.port = Some(1);
    let error = RedisDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Connection, "{error}");
}

#[tokio::test]
async fn non_numeric_database_is_a_configuration_error() {
    let Some(mut config) = integration_config(TestEngine::Redis) else {
        return;
    };
    config.settings.database = Some("main".to_owned());
    let error = RedisDriver::new().connect(&config).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Configuration);
}

#[tokio::test]
async fn database_index_selects_a_separate_keyspace() {
    let Some(mut config) = integration_config(TestEngine::Redis) else {
        return;
    };
    config.settings.database = Some("3".to_owned());
    let other_db = connect_with(&config).await;
    config.settings.database = Some("0".to_owned());
    let default_db = connect_with(&config).await;
    let key = unique_name("relix_redis_db");
    run(kv(other_db.as_ref()), &["SET", &key, "x"]).await;
    let description = kv(default_db.as_ref())
        .describe_key(&KeyName::from(key.as_str()))
        .await
        .unwrap();
    assert_eq!(description.key_type, KeyType::Missing);
    delete_keys(kv(other_db.as_ref()), &[key]).await;
}

#[tokio::test]
async fn scan_walks_many_keys_incrementally_with_a_pattern() {
    let Some(connection) = connect().await else {
        return;
    };
    let kv = kv(connection.as_ref());
    let prefix = unique_name("relix_redis_scan");
    let created: BTreeSet<String> = (0..250).map(|index| format!("{prefix}:{index}")).collect();
    let mut mset = vec!["MSET".to_owned()];
    for key in &created {
        mset.push(key.clone());
        mset.push("v".to_owned());
    }
    assert_eq!(
        kv.execute_command(&mset).await.unwrap(),
        CommandReply::Status("OK".to_owned())
    );

    let pattern = format!("{prefix}:*");
    let mut found = BTreeSet::new();
    let mut cursor = 0;
    let mut pages = 0;
    loop {
        let page = kv.scan_keys(cursor, &pattern, 50).await.unwrap();
        pages += 1;
        found.extend(page.keys.iter().map(KeyName::display));
        if page.is_complete() {
            break;
        }
        cursor = page.next_cursor;
    }
    assert!(pages > 1, "expected several SCAN pages, got {pages}");
    assert_eq!(found, created);

    delete_matching(kv, &pattern).await;
    let empty = kv.scan_keys(0, &pattern, 1000).await.unwrap();
    assert!(empty.keys.is_empty());
}

#[tokio::test]
async fn describe_key_reports_every_type() {
    let Some(connection) = connect().await else {
        return;
    };
    let kv = kv(connection.as_ref());
    let prefix = unique_name("relix_redis_describe");
    let key = |suffix: &str| format!("{prefix}:{suffix}");
    run(kv, &["SET", &key("string"), "hello", "EX", "500"]).await;
    run(kv, &["RPUSH", &key("list"), "a", "b", "c"]).await;
    run(kv, &["SADD", &key("set"), "a", "b"]).await;
    run(
        kv,
        &["ZADD", &key("zset"), "1", "a", "2", "b", "3", "c", "4", "d"],
    )
    .await;
    run(kv, &["HSET", &key("hash"), "f1", "v1", "f2", "v2"]).await;
    run(kv, &["XADD", &key("stream"), "*", "field", "value"]).await;

    let expectations = [
        ("string", KeyType::String, 5),
        ("list", KeyType::List, 3),
        ("set", KeyType::Set, 2),
        ("zset", KeyType::SortedSet, 4),
        ("hash", KeyType::Hash, 2),
        ("stream", KeyType::Stream, 1),
    ];
    for (suffix, key_type, length) in expectations {
        let name = KeyName::from(key(suffix).as_str());
        let description = kv.describe_key(&name).await.unwrap();
        assert_eq!(description.key, name);
        assert_eq!(description.key_type, key_type, "{suffix}");
        assert_eq!(description.length, Some(length), "{suffix}");
        assert!(description.encoding.is_some(), "{suffix}");
        assert!(description.memory_bytes.is_some_and(|bytes| bytes > 0));
        if suffix == "string" {
            assert!(matches!(description.ttl, Ttl::Seconds(seconds) if seconds > 0));
        } else {
            assert_eq!(description.ttl, Ttl::Persistent, "{suffix}");
        }
    }

    let missing = kv
        .describe_key(&KeyName::from(key("missing").as_str()))
        .await
        .unwrap();
    assert_eq!(missing.key_type, KeyType::Missing);
    assert_eq!(missing.ttl, Ttl::Missing);
    assert_eq!(missing.length, None);
    assert_eq!(missing.encoding, None);

    delete_matching(kv, &format!("{prefix}:*")).await;
}

#[tokio::test]
async fn binary_key_names_round_trip() {
    let Some(connection) = connect().await else {
        return;
    };
    let kv = kv(connection.as_ref());
    let mut raw = unique_name("relix_redis_binary").into_bytes();
    raw.extend_from_slice(&[0xff, 0x00, 0xfe]);
    let key = KeyName(raw);
    kv.write(&KeyValueWrite::SetString {
        key: key.clone(),
        value: vec![0, 1, 2],
        ttl_seconds: None,
    })
    .await
    .unwrap();
    let description = kv.describe_key(&key).await.unwrap();
    assert_eq!(description.key_type, KeyType::String);
    let page = kv
        .read_value(&key, &KeyType::String, ValueRange::first(16))
        .await
        .unwrap();
    assert_eq!(
        page,
        ValuePage::String {
            bytes: vec![0, 1, 2],
            total_length: 3
        }
    );
    kv.write(&KeyValueWrite::Delete { key }).await.unwrap();
}

#[tokio::test]
async fn read_value_pages_strings_lists_and_sorted_sets_by_index() {
    let Some(connection) = connect().await else {
        return;
    };
    let kv = kv(connection.as_ref());
    let prefix = unique_name("relix_redis_read_index");
    let string_key = format!("{prefix}:string");
    let list_key = format!("{prefix}:list");
    let zset_key = format!("{prefix}:zset");

    let text = "x".repeat(100);
    run(kv, &["SET", &string_key, &text]).await;
    let page = kv
        .read_value(
            &KeyName::from(string_key.as_str()),
            &KeyType::String,
            ValueRange::first(10),
        )
        .await
        .unwrap();
    assert_eq!(
        page,
        ValuePage::String {
            bytes: b"xxxxxxxxxx".to_vec(),
            total_length: 100
        }
    );
    let length_only = kv
        .read_value(
            &KeyName::from(string_key.as_str()),
            &KeyType::String,
            ValueRange::first(0),
        )
        .await
        .unwrap();
    assert_eq!(
        length_only,
        ValuePage::String {
            bytes: Vec::new(),
            total_length: 100
        }
    );

    let mut rpush = vec!["RPUSH".to_owned(), list_key.clone()];
    rpush.extend((0..25).map(|index| format!("item{index}")));
    kv.execute_command(&rpush).await.unwrap();
    let list = KeyName::from(list_key.as_str());
    let mut cursor = Some(0);
    let mut collected = Vec::new();
    let mut starts = Vec::new();
    while let Some(start) = cursor {
        let page = kv
            .read_value(
                &list,
                &KeyType::List,
                ValueRange {
                    cursor: start,
                    count: 10,
                },
            )
            .await
            .unwrap();
        let ValuePage::List {
            items,
            start,
            next_cursor,
        } = page
        else {
            panic!("expected a list page");
        };
        starts.push(start);
        collected.extend(items);
        cursor = next_cursor;
    }
    assert_eq!(starts, vec![0, 10, 20]);
    assert_eq!(collected.len(), 25);
    assert_eq!(collected[24], b"item24".to_vec());

    let mut zadd = vec!["ZADD".to_owned(), zset_key.clone()];
    for index in 0..12 {
        zadd.push(format!("{index}.5"));
        zadd.push(format!("member{index:02}"));
    }
    kv.execute_command(&zadd).await.unwrap();
    let zset = KeyName::from(zset_key.as_str());
    let first = kv
        .read_value(&zset, &KeyType::SortedSet, ValueRange::first(5))
        .await
        .unwrap();
    let ValuePage::SortedSet {
        entries,
        start,
        next_cursor,
    } = first
    else {
        panic!("expected a sorted set page");
    };
    assert_eq!(start, 0);
    assert_eq!(next_cursor, Some(5));
    assert_eq!(entries[0], (b"member00".to_vec(), 0.5));
    assert_eq!(entries.len(), 5);
    let last = kv
        .read_value(
            &zset,
            &KeyType::SortedSet,
            ValueRange {
                cursor: 10,
                count: 5,
            },
        )
        .await
        .unwrap();
    let ValuePage::SortedSet {
        entries,
        next_cursor,
        ..
    } = last
    else {
        panic!("expected a sorted set page");
    };
    assert_eq!(next_cursor, None);
    assert_eq!(
        entries,
        vec![(b"member10".to_vec(), 10.5), (b"member11".to_vec(), 11.5)]
    );

    delete_keys(kv, &[string_key, list_key, zset_key]).await;
}

#[tokio::test]
async fn read_value_pages_sets_and_hashes_by_cursor() {
    let Some(connection) = connect().await else {
        return;
    };
    let kv = kv(connection.as_ref());
    let prefix = unique_name("relix_redis_read_scan");
    let set_key = format!("{prefix}:set");
    let hash_key = format!("{prefix}:hash");

    let mut sadd = vec!["SADD".to_owned(), set_key.clone()];
    sadd.extend((0..300).map(|index| format!("member{index}")));
    kv.execute_command(&sadd).await.unwrap();
    let mut hset = vec!["HSET".to_owned(), hash_key.clone()];
    for index in 0..600 {
        hset.push(format!("field{index}"));
        hset.push(format!("value{index}"));
    }
    kv.execute_command(&hset).await.unwrap();

    let set = KeyName::from(set_key.as_str());
    let mut members = BTreeSet::new();
    let mut cursor = Some(0);
    let mut pages = 0;
    while let Some(position) = cursor {
        let page = kv
            .read_value(
                &set,
                &KeyType::Set,
                ValueRange {
                    cursor: position,
                    count: 50,
                },
            )
            .await
            .unwrap();
        let ValuePage::Set {
            members: batch,
            next_cursor,
        } = page
        else {
            panic!("expected a set page");
        };
        members.extend(batch);
        cursor = next_cursor;
        pages += 1;
    }
    assert!(pages > 1);
    assert_eq!(members.len(), 300);

    let hash = KeyName::from(hash_key.as_str());
    let mut fields = std::collections::BTreeMap::new();
    let mut cursor = Some(0);
    let mut pages = 0;
    while let Some(position) = cursor {
        let page = kv
            .read_value(
                &hash,
                &KeyType::Hash,
                ValueRange {
                    cursor: position,
                    count: 50,
                },
            )
            .await
            .unwrap();
        let ValuePage::Hash {
            entries,
            next_cursor,
        } = page
        else {
            panic!("expected a hash page");
        };
        fields.extend(entries);
        cursor = next_cursor;
        pages += 1;
    }
    assert!(pages > 1);
    assert_eq!(fields.len(), 600);
    assert_eq!(
        fields.get(b"field7".as_slice()).cloned(),
        Some(b"value7".to_vec())
    );

    delete_keys(kv, &[set_key, hash_key]).await;
}

#[tokio::test]
async fn read_value_returns_the_first_stream_entries_and_flags_unsupported_types() {
    let Some(connection) = connect().await else {
        return;
    };
    let kv = kv(connection.as_ref());
    let stream_key = unique_name("relix_redis_stream");
    for index in 0..15 {
        let value = format!("v{index}");
        run(kv, &["XADD", &stream_key, "*", "index", &value]).await;
    }
    let page = kv
        .read_value(
            &KeyName::from(stream_key.as_str()),
            &KeyType::Stream,
            ValueRange::first(10),
        )
        .await
        .unwrap();
    let ValuePage::Stream {
        entries,
        next_cursor,
    } = page
    else {
        panic!("expected a stream page");
    };
    assert_eq!(entries.len(), 10);
    assert_eq!(next_cursor, None);
    assert!(entries[0].id.contains('-'));
    assert_eq!(entries[0].fields, vec![(b"index".to_vec(), b"v0".to_vec())]);

    let unsupported = kv
        .read_value(
            &KeyName::from(stream_key.as_str()),
            &KeyType::Other("ReJSON-RL".to_owned()),
            ValueRange::first(10),
        )
        .await
        .unwrap();
    assert_eq!(
        unsupported,
        ValuePage::Unsupported {
            key_type: "ReJSON-RL".to_owned()
        }
    );

    delete_keys(kv, &[stream_key]).await;
}

#[tokio::test]
async fn every_write_operation_reaches_the_server() {
    let Some(connection) = connect().await else {
        return;
    };
    let kv = kv(connection.as_ref());
    let prefix = unique_name("relix_redis_write");
    let name = |suffix: &str| KeyName::from(format!("{prefix}:{suffix}").as_str());
    let string = name("string");
    let hash = name("hash");
    let list = name("list");
    let set = name("set");
    let zset = name("zset");
    let renamed = name("renamed");

    kv.write(&KeyValueWrite::SetString {
        key: string.clone(),
        value: b"value".to_vec(),
        ttl_seconds: Some(300),
    })
    .await
    .unwrap();
    let described = kv.describe_key(&string).await.unwrap();
    assert!(matches!(described.ttl, Ttl::Seconds(seconds) if seconds > 0 && seconds <= 300));
    kv.write(&KeyValueWrite::Persist {
        key: string.clone(),
    })
    .await
    .unwrap();
    assert_eq!(kv.describe_key(&string).await.unwrap().ttl, Ttl::Persistent);
    kv.write(&KeyValueWrite::Expire {
        key: string.clone(),
        seconds: 120,
    })
    .await
    .unwrap();
    assert!(matches!(
        kv.describe_key(&string).await.unwrap().ttl,
        Ttl::Seconds(seconds) if seconds <= 120
    ));
    let missing_expire = kv
        .write(&KeyValueWrite::Expire {
            key: name("absent"),
            seconds: 5,
        })
        .await
        .unwrap_err();
    assert_eq!(missing_expire.kind, ErrorKind::Query);
    kv.write(&KeyValueWrite::Rename {
        key: string.clone(),
        new_key: renamed.clone(),
    })
    .await
    .unwrap();
    assert_eq!(
        kv.describe_key(&string).await.unwrap().key_type,
        KeyType::Missing
    );
    assert_eq!(
        kv.describe_key(&renamed).await.unwrap().key_type,
        KeyType::String
    );

    for (field, value) in [("a", "1"), ("b", "2")] {
        kv.write(&KeyValueWrite::HashSet {
            key: hash.clone(),
            field: field.as_bytes().to_vec(),
            value: value.as_bytes().to_vec(),
        })
        .await
        .unwrap();
    }
    kv.write(&KeyValueWrite::HashDelete {
        key: hash.clone(),
        field: b"a".to_vec(),
    })
    .await
    .unwrap();
    assert_eq!(
        kv.read_value(&hash, &KeyType::Hash, ValueRange::first(10))
            .await
            .unwrap(),
        ValuePage::Hash {
            entries: vec![(b"b".to_vec(), b"2".to_vec())],
            next_cursor: None
        }
    );

    for (value, tail) in [("middle", true), ("last", true), ("first", false)] {
        kv.write(&KeyValueWrite::ListPush {
            key: list.clone(),
            value: value.as_bytes().to_vec(),
            tail,
        })
        .await
        .unwrap();
    }
    kv.write(&KeyValueWrite::ListSet {
        key: list.clone(),
        index: 1,
        value: b"center".to_vec(),
    })
    .await
    .unwrap();
    assert_eq!(
        kv.read_value(&list, &KeyType::List, ValueRange::first(10))
            .await
            .unwrap(),
        ValuePage::List {
            items: vec![b"first".to_vec(), b"center".to_vec(), b"last".to_vec()],
            start: 0,
            next_cursor: None
        }
    );
    let out_of_range = kv
        .write(&KeyValueWrite::ListSet {
            key: list.clone(),
            index: 99,
            value: b"x".to_vec(),
        })
        .await
        .unwrap_err();
    assert_eq!(out_of_range.kind, ErrorKind::Query);

    for member in ["keep", "drop"] {
        kv.write(&KeyValueWrite::SetAdd {
            key: set.clone(),
            member: member.as_bytes().to_vec(),
        })
        .await
        .unwrap();
    }
    kv.write(&KeyValueWrite::SetRemove {
        key: set.clone(),
        member: b"drop".to_vec(),
    })
    .await
    .unwrap();
    assert_eq!(
        kv.read_value(&set, &KeyType::Set, ValueRange::first(10))
            .await
            .unwrap(),
        ValuePage::Set {
            members: vec![b"keep".to_vec()],
            next_cursor: None
        }
    );

    for (member, score) in [("low", -1.25), ("high", 99.0)] {
        kv.write(&KeyValueWrite::SortedSetAdd {
            key: zset.clone(),
            member: member.as_bytes().to_vec(),
            score,
        })
        .await
        .unwrap();
    }
    kv.write(&KeyValueWrite::SortedSetRemove {
        key: zset.clone(),
        member: b"high".to_vec(),
    })
    .await
    .unwrap();
    assert_eq!(
        kv.read_value(&zset, &KeyType::SortedSet, ValueRange::first(10))
            .await
            .unwrap(),
        ValuePage::SortedSet {
            entries: vec![(b"low".to_vec(), -1.25)],
            start: 0,
            next_cursor: None
        }
    );

    let wrong_type = kv
        .write(&KeyValueWrite::HashSet {
            key: list.clone(),
            field: b"f".to_vec(),
            value: b"v".to_vec(),
        })
        .await
        .unwrap_err();
    assert_eq!(wrong_type.kind, ErrorKind::Query);
    assert!(wrong_type.message.contains("WRONGTYPE"), "{wrong_type}");

    for key in [renamed, hash, list, set, zset] {
        kv.write(&KeyValueWrite::Delete { key: key.clone() })
            .await
            .unwrap();
        assert_eq!(
            kv.describe_key(&key).await.unwrap().key_type,
            KeyType::Missing
        );
    }
}

#[tokio::test]
async fn execute_command_returns_replies_and_server_errors() {
    let Some(connection) = connect().await else {
        return;
    };
    let kv = kv(connection.as_ref());
    let key = unique_name("relix_redis_console");
    assert_eq!(
        run(kv, &["set", &key, "hello"]).await,
        CommandReply::Status("OK".to_owned())
    );
    assert_eq!(
        run(kv, &["GET", &key]).await,
        CommandReply::Bulk(b"hello".to_vec())
    );
    assert_eq!(run(kv, &["STRLEN", &key]).await, CommandReply::Integer(5));
    assert_eq!(
        run(kv, &["GET", "relix_missing_key_xyz"]).await,
        CommandReply::Nil
    );
    assert_eq!(
        run(kv, &["PING"]).await,
        CommandReply::Status("PONG".to_owned())
    );
    assert!(matches!(
        run(kv, &["LPUSH", &key, "x"]).await,
        CommandReply::Error(message) if message.starts_with("WRONGTYPE")
    ));
    assert!(matches!(
        run(kv, &["NOSUCHCOMMAND"]).await,
        CommandReply::Error(message) if message.starts_with("ERR")
    ));
    let rejected = kv
        .execute_command(&arguments(&["BLPOP", &key, "0"]))
        .await
        .unwrap_err();
    assert_eq!(rejected.kind, ErrorKind::Unsupported);
    let subscribe = kv
        .execute_command(&arguments(&["SUBSCRIBE", "news"]))
        .await
        .unwrap_err();
    assert_eq!(subscribe.kind, ErrorKind::Unsupported);
    let empty = kv.execute_command(&[]).await.unwrap_err();
    assert_eq!(empty.kind, ErrorKind::Configuration);
    connection.ping().await.unwrap();
    delete_keys(kv, &[key]).await;
}

#[tokio::test]
async fn server_details_include_info_and_dbsize() {
    let Some(connection) = connect().await else {
        return;
    };
    let details = kv(connection.as_ref()).server_details().await.unwrap();
    let lookup = |name: &str| {
        details
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(
        lookup("server.redis_version"),
        Some(connection.server_info().version)
    );
    assert!(lookup("memory.used_memory").is_some());
    assert!(lookup("dbsize").is_some_and(|size| size.parse::<u64>().is_ok()));
}

#[tokio::test]
async fn tls_against_a_plaintext_server_fails_cleanly() {
    let Some(mut config) = integration_config(TestEngine::Redis) else {
        return;
    };
    config.settings.connect_timeout_secs = 2;
    for mode in [TlsMode::Require, TlsMode::VerifyFull] {
        config.settings.tls.mode = mode;
        let error = RedisDriver::new().connect(&config).await.err().unwrap();
        assert!(
            matches!(
                error.kind,
                ErrorKind::Tls | ErrorKind::Connection | ErrorKind::Timeout
            ),
            "{mode:?}: {error}"
        );
        if error.kind == ErrorKind::Timeout {
            assert!(error.detail.is_some_and(|detail| detail.contains("TLS")));
        }
    }
}
