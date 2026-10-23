/*
 * @author t0bx
 * @since 23.10.2026
 */

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn parse(value: &str) -> Option<Self> {
                Uuid::parse_str(value).ok().map(Self)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

uuid_id!(ProjectId);
uuid_id!(DataSourceId);
uuid_id!(ServerConfigId);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DriverId(String);

impl DriverId {
    pub const POSTGRES: &'static str = "postgres";
    pub const MYSQL: &'static str = "mysql";
    pub const MARIADB: &'static str = "mariadb";
    pub const SQLITE: &'static str = "sqlite";
    pub const REDIS: &'static str = "redis";
    pub const COCKROACHDB: &'static str = "cockroachdb";
    pub const YUGABYTEDB: &'static str = "yugabytedb";
    pub const TIMESCALEDB: &'static str = "timescaledb";
    pub const TIDB: &'static str = "tidb";
    pub const VALKEY: &'static str = "valkey";
    pub const MSSQL: &'static str = "mssql";
    pub const DUCKDB: &'static str = "duckdb";
    pub const ORACLE: &'static str = "oracle";
    pub const CLICKHOUSE: &'static str = "clickhouse";
    pub const SNOWFLAKE: &'static str = "snowflake";
    pub const BIGQUERY: &'static str = "bigquery";
    pub const TRINO: &'static str = "trino";
    pub const MONGODB: &'static str = "mongodb";
    pub const CASSANDRA: &'static str = "cassandra";
    pub const SCYLLADB: &'static str = "scylladb";
    pub const INFLUXDB: &'static str = "influxdb";
    pub const NEO4J: &'static str = "neo4j";
    pub const ELASTICSEARCH: &'static str = "elasticsearch";
    pub const OPENSEARCH: &'static str = "opensearch";
    pub const DYNAMODB: &'static str = "dynamodb";

    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DriverId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for DriverId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_roundtrip_through_strings() {
        let id = DataSourceId::new();
        assert_eq!(DataSourceId::parse(&id.to_string()), Some(id));
        assert_eq!(DataSourceId::parse("not-a-uuid"), None);
    }
}
