# Supported database engines

All engines are compiled into the `relix` binary; nothing is downloaded at runtime. Engines
that need a native component say so in the driver list (`Space d`) and refuse to connect
with installation instructions when it is missing.

| Engine | Driver id | Data model | Console language | Default port | SSH tunnel | Verified against |
|---|---|---|---|---|---|---|
| PostgreSQL | `postgres` | relational | PostgreSQL SQL | 5432 | yes | PostgreSQL 17 |
| CockroachDB | `cockroachdb` | relational | PostgreSQL SQL | 26257 | yes | CockroachDB 26.3 |
| YugabyteDB (YSQL) | `yugabytedb` | relational | PostgreSQL SQL | 5433 | yes | YugabyteDB 2026.1 |
| TimescaleDB | `timescaledb` | relational | PostgreSQL SQL | 5432 | yes | TimescaleDB 2.31 / PG 17 |
| MySQL | `mysql` | relational | MySQL SQL | 3306 | yes | MySQL 8.4 |
| MariaDB | `mariadb` | relational | MySQL SQL | 3306 | yes | MariaDB 11 |
| TiDB | `tidb` | relational | MySQL SQL | 4000 | yes | TiDB 8.5 |
| SQLite | `sqlite` | relational | SQLite SQL | file | n/a | bundled SQLite |
| Microsoft SQL Server | `mssql` | relational | T-SQL | 1433 | yes | Azure SQL Edge (SQL Server engine) |
| Oracle Database | `oracle` | relational | Oracle SQL / PL/SQL | 1521 | yes | Oracle Database 23ai Free |
| DuckDB | `duckdb` | relational | DuckDB SQL | file | n/a | bundled DuckDB 1.x |
| ClickHouse | `clickhouse` | relational (analytics) | ClickHouse SQL | 8123 | yes | ClickHouse 25.8 |
| Trino | `trino` | relational (federated) | Trino SQL | 8080 | yes | Trino 476 |
| Snowflake | `snowflake` | relational (warehouse) | Snowflake SQL | 443 | yes | protocol tests only — needs a real account |
| Google BigQuery | `bigquery` | relational (warehouse) | GoogleSQL | cloud | no | BigQuery emulator + protocol tests |
| Redis | `redis` | key-value | Redis commands | 6379 | yes | Redis 7 |
| Valkey | `valkey` | key-value | Redis commands | 6379 | yes | Valkey 8.1 |
| MongoDB | `mongodb` | document | MongoDB commands (Extended JSON) | 27017 | yes | MongoDB 8 |
| Amazon DynamoDB | `dynamodb` | document | PartiQL | cloud | no | DynamoDB Local |
| Elasticsearch | `elasticsearch` | search | Dev-Tools requests | 9200 | yes | Elasticsearch 8.15 |
| OpenSearch | `opensearch` | search | Dev-Tools requests | 9200 | yes | OpenSearch 2.19 |
| Apache Cassandra | `cassandra` | wide-column | CQL | 9042 | yes | Cassandra 5 |
| ScyllaDB | `scylladb` | wide-column | CQL | 9042 | yes | ScyllaDB 2026.3 |
| Neo4j | `neo4j` | graph | Cypher | 7687 | yes | Neo4j 5.26 Community |
| InfluxDB 3 | `influxdb` | time-series | SQL (InfluxQL optional) | 8181 | yes | InfluxDB 3 Core 3.12 |

"Verified against" lists the server versions the automated integration tests ran against.
Snowflake has no local server; its driver is tested against a mock of the documented SQL API
and still needs confirmation against a real account. BigQuery's real authentication is only
covered by protocol tests.

## How each data model is presented

* **Relational, wide-column, time-series** — explorer of databases/keyspaces → schemas →
  tables/views with columns, keys and indexes; editable data grid where the engine supports
  correct single-row updates; SQL/CQL console.
* **Graph (Neo4j)** — explorer of node labels and relationship types with their observed
  properties, indexes and constraints; nodes and relationships browse in the grid; Cypher
  console with nodes, relationships and paths rendered as JSON.
* **Key-value (Redis, Valkey)** — key browser with type-aware value views and a command
  console.
* **Document and search (MongoDB, DynamoDB, Elasticsearch, OpenSearch)** — explorer of
  namespaces → collections (indices, aliases, data streams, tables); document browser with
  native JSON filters, paging, JSON editing, collection details and index management;
  native console.

Details per engine: [relational](relational.md), [analytics and cloud](analytics.md),
[NoSQL, graph and time-series](nosql.md). Build and deployment notes:
[deployment](deployment.md).
