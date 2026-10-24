# Key-value, document, search, wide-column, graph and time-series engines

## Redis and Valkey

Key browser (SCAN based, pattern search), string/hash/list/set/sorted set/stream values,
TTL inspection and editing, key creation, deletion and renaming, and a command console.
Database index in the *Database* field, ACL user in *User*. Valkey is detected automatically
and shown as Valkey. TLS `verify-full` is refused through SSH tunnels (see
[SSH tunnels](../connectivity/ssh-tunnels.md#tls-through-a-tunnel)).

## MongoDB

* Fields: host, port, user, password, database (default for browsing and the console).
* Properties: `uri` (full connection string incl. `mongodb+srv://`; host/port are ignored,
  the password field fills in a missing password; not combinable with SSH tunnels),
  `authSource` (default `admin`), `authMechanism`, `replicaSet`, `directConnection`.
* TLS: `require` (encrypted, unverified) or `verify-full` (Mozilla roots + CA file);
  `prefer` connects in plain text because MongoDB has no opportunistic TLS. `verify-full`
  through an SSH tunnel is refused (the client verifies the dialed host).
* Document browser: filter = MongoDB query in Extended JSON, e.g. `{"age": {"$gt": 30}}`;
  documents are shown and edited as relaxed Extended JSON (`{"$oid": …}`, `{"$date": …}`,
  64-bit integers as `{"$numberLong": …}` so they keep their type).
* Index definitions: `{"keys": {"email": 1}, "options": {"unique": true}}`.
* Console: one command document per statement (`{"find": "users", "filter": {…}}`,
  `{"aggregate": "orders", "pipeline": […]}`, `{"insert": …}`), `use <db>` switches the
  database, a `"$db"` field targets another one. Cursors are followed up to the row limit.
  Cancellation kills your own operation on the server (MongoDB 4.4+).

## Amazon DynamoDB

* Cloud target. Credentials from the AWS chain (environment, profiles, SSO,
  credential_process, ECS/IMDS) or explicit keys (user = access key id, password = secret).
* Properties: `region`, `profile`, `endpoint_url` (DynamoDB Local, VPC endpoints).
* Document browser: filter = native Query/Scan parameters, e.g.
  `{"KeyConditionExpression": "pk = :pk", "ExpressionAttributeValues": {":pk": "user#1"}}`
  (with `KeyConditionExpression` a Query runs, otherwise a Scan; `IndexName` selects an index).
  Values map to JSON; numbers that JSON cannot express exactly, binary and sets use
  `{"$N": …}`, `{"$B": …}`, `{"$SS": …}`, `{"$NS": …}`, `{"$BS": …}`.
* Index management creates and deletes global secondary indexes (asynchronous on AWS).
* Console: PartiQL. Cancellation only stops waiting (DynamoDB has no server-side cancel).
* Totals are not reported; item counts are DynamoDB's approximation.

## Elasticsearch and OpenSearch

* Fields: host, port (9200), user, password, TLS mode and CA file. Property `auth` = `basic`
  (default), `api_key` (key in the password field; with a user name, user = key id),
  `bearer` or `none`; `show_hidden`, `track_total_hits`, `path_prefix`.
* Collections are indices, aliases and data streams; details show mappings, settings,
  health and shard information.
* Document browser: filter = Query DSL `query` object (empty = `match_all`); paging uses
  `from`/`size` and continues with `search_after` beyond the result window; replace and delete
  use optimistic concurrency (`if_seq_no`/`if_primary_term`). Creating an index takes
  `{"name": …, "mappings": …, "settings": …}`.
* Console: Dev-Tools style requests — `METHOD /path` followed by an optional JSON body
  (NDJSON for `_bulk`/`_msearch`); hits become rows, other responses tables or key/value rows.
  Cancellation cancels the request's server task via `X-Opaque-Id`.

## Apache Cassandra and ScyllaDB

* Fields: host, port (9042), user, password, database = keyspace. The product is detected
  automatically.
* Keyspaces, tables and materialized views; columns show their kind (partition key,
  clustering column with order, static, regular).
* Console: CQL with paging up to the row limit; `USE` switches the keyspace.
* Grid: CQL has no OFFSET, so deep pages read and skip the earlier rows; filters are WHERE
  clauses (add `ALLOW FILTERING` yourself). Edits use lightweight transactions (`IF col =
  original`, `IF NOT EXISTS`, `IF EXISTS`) and are applied one statement at a time — there are
  no multi-statement transactions.
* All traffic goes through the configured node (no driver-side load balancing or failover),
  which makes tunnels and NAT work. Cancellation stops waiting; the coordinator may still
  finish the request.

## Neo4j

* Bolt, port 7687. Fields: host, port, user, password, database.
* Explorer: node labels and relationship types with counts, observed properties and types,
  indexes and constraints. Labels and relationship types open in the grid (nodes with their
  properties as columns, relationships with type and start/end ids); property edits keep the
  stored type and are applied in one transaction.
* Console: Cypher; nodes, relationships and paths are rendered as JSON. `BEGIN`/`COMMIT`/
  `ROLLBACK` run explicit transactions; cancellation terminates auto-commit transactions on the
  server.
* Limitations: the Bolt 4.4 client shows legacy numeric ids in console results — return
  `elementId(n)` for element ids; `RETURN *` columns are sorted alphabetically.

## InfluxDB 3

* HTTP API v3, port 8181. Token in the password field (sent as Bearer; empty for servers
  without authentication). Properties: `database`, `query_language` = `sql` (default) or
  `influxql`, `path_prefix`.
* Explorer: databases, the `iox` schema with measurements, `system` and `information_schema`;
  columns marked tag, field or time. Measurements browse newest first; filter with time ranges
  such as `time > now() - INTERVAL '1 hour'`. Tables are read-only.
