# Relational engines

All relational engines support the SQL console (statement under cursor, selection or whole
script; row limit with streaming), the explorer (databases, schemas, tables, views,
routines, sequences, columns, primary/foreign keys, indexes, definitions) and server-side
cancellation unless noted. Grid edits run in one transaction, and every update/delete must
affect exactly one row (guarded by the original values) or everything is rolled back.

## PostgreSQL family

Fields: host, port, user, password, database, TLS mode and CA file. TLS through SSH tunnels
verifies against the original host name.

* **PostgreSQL** — the reference implementation.
* **CockroachDB** — defaults: user `root`, database `defaultdb`. The hidden `rowid` column is
  not shown, definitions use `SHOW CREATE`, error positions are read from Cockroach's caret
  diagrams. Tables whose only key is the implicit `rowid` have no editable row identity.
* **YugabyteDB (YSQL)** — defaults: user `yugabyte`. Concurrent DDL can fail with
  serialization errors (engine behaviour).
* **TimescaleDB** — hypertables are marked `hypertable` in the explorer, their DDL includes the
  `create_hypertable`/`add_dimension` calls; continuous aggregates appear as materialized
  views. Compression and retention policies are not part of the generated DDL.

Selecting the PostgreSQL driver for a CockroachDB server (or vice versa) works and is
reported as e.g. "CockroachDB (connected via PostgreSQL driver)"; introspection follows the
detected engine.

## MySQL family

Fields: host, port, user, password, database (optional default), TLS mode and CA file.

* **MySQL**, **MariaDB** — as before.
* **TiDB** — default port 4000. `AUTO_RANDOM` columns are recognised as generated keys.
  Cancellation uses `KILL QUERY` on `CONNECTION_ID()` and relies on TiDB's global kill (default
  since 6.1). TiDB has no stored procedures, so routines are not listed.

## SQLite

File databases (`:memory:` is possible), *Create file* creates missing files.

## Microsoft SQL Server

* Fields: host, port (1433), user, password (SQL Server logins), database, TLS mode and CA
  file. Named instances need an explicit port. Windows/NTLM/Azure AD authentication is not
  supported.
* TLS: `disable` (servers that force encryption still get TLS without certificate checks),
  `prefer`/`require` (encrypted, certificate not checked), `verify-full` (OS trust store or the
  CA file: one certificate in `.pem`, `.crt` or `.der`). Through SSH tunnels the certificate is
  verified against the original host. Azure SQL redirects are followed, except through a tunnel.
* Console: one session connection, so `BEGIN TRAN`, temp tables and `SET` carry over. When a
  batch returns several result sets, the first is shown. `PRINT` messages are not shown.
* Cancellation kills the session (`KILL <spid>`, needs `ALTER ANY CONNECTION`); an open
  transaction is rolled back and the next statement opens a new session.
* Limitations: `money` is shown as a floating point number; edits guarded on `text`, `ntext`
  or `image` columns fail because SQL Server cannot compare them.

## Oracle Database

* **Requires the Oracle Instant Client** (Basic or Basic Light, 64-bit) at runtime; see
  [deployment](deployment.md). Without it, connecting fails with installation hints.
* Fields: host, port (1521), user, password, **database = service name** (e.g. `FREEPDB1`).
  Property `client_lib_dir` points at the Instant Client directory (needed on macOS).
* Schemas are the non-Oracle-maintained users plus your own; packages are listed with the
  routines (comment `package`) and their DDL includes the body.
* Transactions are implicit, as in Oracle: DML stays uncommitted until you run `COMMIT`; DDL
  commits itself; closing the connection rolls back. Grid edits commit explicitly.
* TLS (TCPS with wallets) is not supported; `require`/`verify-full` are refused. Use an SSH
  tunnel or Oracle Native Network Encryption. TNS aliases, SID-only connections and
  SYSDBA/external authentication are not supported.
* Limitations: unconstrained `NUMBER` values are shown as text; edits guarded on LOB columns
  fail; cancellation may only take effect when a sleeping statement wakes up.

## DuckDB

* File databases (or `:memory:`); property `access_mode` = `automatic` (default),
  `read_only` (to share a file with other processes) or `read_write`.
* All DuckDB types are rendered like DuckDB's own `::VARCHAR` cast (HUGEINT, DECIMAL,
  temporal types, LIST, ARRAY, STRUCT, MAP, UNION, ENUM, …). `TIME WITH TIME ZONE` results ask
  you to cast to `VARCHAR`. The ICU extension is not bundled, so `TIMESTAMPTZ` is shown in UTC.
* Macros are listed as routines; their default parameter values are not part of the
  reconstructed definition.
