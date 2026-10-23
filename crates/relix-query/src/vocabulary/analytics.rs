/*
 * @author t0bx
 * @since 23.10.2026
 */

pub(super) const CLICKHOUSE_KEYWORDS: &[&str] = &[
    "PREWHERE",
    "FINAL",
    "SAMPLE",
    "ARRAY",
    "ENGINE",
    "SETTINGS",
    "FORMAT",
    "TTL",
    "OPTIMIZE",
    "SYSTEM",
    "ALIAS",
    "MATERIALIZED",
    "PARTITION",
    "GLOBAL",
    "ANY",
    "ASOF",
];

pub(super) const CLICKHOUSE_FUNCTIONS: &[&str] = &[
    "toDate",
    "toDateTime",
    "toStartOfHour",
    "toStartOfDay",
    "now",
    "today",
    "uniq",
    "uniqExact",
    "groupArray",
    "arrayJoin",
    "quantile",
    "countIf",
    "sumIf",
    "argMax",
    "argMin",
    "formatDateTime",
    "JSONExtractString",
];

pub(super) const CLICKHOUSE_TYPES: &[&str] = &[
    "UInt8",
    "UInt16",
    "UInt32",
    "UInt64",
    "Int8",
    "Int16",
    "Int32",
    "Int64",
    "Float32",
    "Float64",
    "String",
    "FixedString",
    "DateTime64",
    "LowCardinality",
    "Nullable",
    "Array",
    "Map",
    "Tuple",
    "UUID",
    "Enum8",
];

pub(super) const DUCKDB_KEYWORDS: &[&str] = &[
    "PIVOT",
    "UNPIVOT",
    "QUALIFY",
    "ATTACH",
    "DETACH",
    "COPY",
    "EXPORT",
    "IMPORT",
    "INSTALL",
    "LOAD",
    "PRAGMA",
    "SUMMARIZE",
    "DESCRIBE",
    "MACRO",
];

pub(super) const DUCKDB_FUNCTIONS: &[&str] = &[
    "read_csv",
    "read_parquet",
    "read_json",
    "list_value",
    "struct_pack",
    "epoch_ms",
    "strftime",
    "regexp_matches",
    "string_agg",
    "unnest",
    "range",
    "generate_series",
];

pub(super) const DUCKDB_TYPES: &[&str] = &[
    "HUGEINT",
    "UBIGINT",
    "UINTEGER",
    "BLOB",
    "LIST",
    "STRUCT",
    "MAP",
    "UNION",
    "UUID",
    "JSON",
    "TIMESTAMPTZ",
];

pub(super) const TRINO_KEYWORDS: &[&str] = &[
    "CATALOGS",
    "SCHEMAS",
    "SHOW",
    "EXPLAIN",
    "ANALYZE",
    "UNNEST",
    "TABLESAMPLE",
    "LATERAL",
    "FETCH",
    "OFFSET",
    "ROWS",
    "ONLY",
    "WINDOW",
];

pub(super) const TRINO_FUNCTIONS: &[&str] = &[
    "approx_distinct",
    "approx_percentile",
    "array_agg",
    "date_trunc",
    "date_add",
    "date_diff",
    "from_unixtime",
    "to_unixtime",
    "json_extract",
    "json_extract_scalar",
    "regexp_like",
    "cardinality",
    "element_at",
];

pub(super) const TRINO_TYPES: &[&str] = &[
    "VARCHAR",
    "VARBINARY",
    "DOUBLE",
    "REAL",
    "ARRAY",
    "MAP",
    "ROW",
    "JSON",
    "IPADDRESS",
    "UUID",
    "TIMESTAMP WITH TIME ZONE",
];

pub(super) const SNOWFLAKE_KEYWORDS: &[&str] = &[
    "WAREHOUSE",
    "STAGE",
    "STREAM",
    "TASK",
    "PIPE",
    "CLONE",
    "QUALIFY",
    "FLATTEN",
    "LATERAL",
    "UNDROP",
    "COPY",
    "PUT",
    "GET",
    "SHOW",
    "DESCRIBE",
];

pub(super) const SNOWFLAKE_FUNCTIONS: &[&str] = &[
    "IFF",
    "NVL",
    "ZEROIFNULL",
    "TRY_TO_NUMBER",
    "TO_VARIANT",
    "PARSE_JSON",
    "OBJECT_CONSTRUCT",
    "ARRAY_AGG",
    "DATEADD",
    "DATEDIFF",
    "CURRENT_WAREHOUSE",
    "CURRENT_ROLE",
    "LISTAGG",
];

pub(super) const SNOWFLAKE_TYPES: &[&str] = &[
    "VARIANT",
    "OBJECT",
    "ARRAY",
    "NUMBER",
    "TIMESTAMP_NTZ",
    "TIMESTAMP_LTZ",
    "TIMESTAMP_TZ",
    "GEOGRAPHY",
    "BINARY",
];

pub(super) const BIGQUERY_KEYWORDS: &[&str] = &[
    "STRUCT",
    "UNNEST",
    "QUALIFY",
    "PARTITION",
    "CLUSTER",
    "OPTIONS",
    "EXCEPT",
    "REPLACE",
    "WINDOW",
    "MERGE",
    "SAFE",
    "DECLARE",
];

pub(super) const BIGQUERY_FUNCTIONS: &[&str] = &[
    "SAFE_CAST",
    "FORMAT_DATE",
    "FORMAT_TIMESTAMP",
    "PARSE_DATE",
    "DATE_TRUNC",
    "TIMESTAMP_TRUNC",
    "GENERATE_ARRAY",
    "ARRAY_AGG",
    "STRING_AGG",
    "APPROX_COUNT_DISTINCT",
    "JSON_VALUE",
    "TO_JSON_STRING",
    "FROM_HEX",
];

pub(super) const BIGQUERY_TYPES: &[&str] = &[
    "INT64",
    "FLOAT64",
    "NUMERIC",
    "BIGNUMERIC",
    "BOOL",
    "STRING",
    "BYTES",
    "DATETIME",
    "GEOGRAPHY",
    "ARRAY",
    "STRUCT",
    "JSON",
];

pub(super) const INFLUX_KEYWORDS: &[&str] = &[
    "INTERVAL",
    "EXPLAIN",
    "SHOW",
    "TAGS",
    "FIELDS",
    "MEASUREMENTS",
    "SERIES",
    "RETENTION",
];

pub(super) const INFLUX_FUNCTIONS: &[&str] = &[
    "date_bin",
    "date_bin_gapfill",
    "date_trunc",
    "now",
    "to_timestamp",
    "selector_first",
    "selector_last",
    "selector_max",
    "locf",
    "interpolate",
    "approx_percentile_cont",
];

pub(super) const INFLUX_TYPES: &[&str] = &["TIMESTAMP", "DOUBLE", "BIGINT", "UTF8", "BOOLEAN"];
