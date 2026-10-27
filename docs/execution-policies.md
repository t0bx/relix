# Execution policies

Every data source has an execution policy that decides what RELIX lets you write to it. The
policy is checked in one place (the `relix-safety` crate) by every workflow that changes
data: editor statements and scripts, grid commits, key-value edits and console commands,
and document/index writes. Future migration, import and CLI workflows use the same check.

| Policy | Reads | Writes | Default for |
|---|---|---|---|
| `read-only` | allowed | refused, also by the database where the driver supports it | `production` |
| `confirm writes` | allowed | every write asks for confirmation first | `staging` |
| `unrestricted` | allowed | allowed; risky statements still ask on staging/production when `confirm_destructive` is on | `none`, `development`, `testing` |

Choose the policy in the data source dialog (*Execution*). *environment default* follows the
table above, so changing the environment changes the policy too. The current policy is
shown as a badge in the header and under *execution* in the data source details.

## What counts as a write

Statements are classified by their leading keywords, ignoring comments and string literals:

* `SELECT`, `VALUES`, `TABLE`, `SHOW`, `EXPLAIN`, `DESCRIBE`, `PRAGMA` are reads, and so are
  session and transaction control (`SET`, `USE`, `BEGIN`, `COMMIT`, `ROLLBACK`).
* `EXPLAIN ANALYZE` runs its statement, so it is classified as that statement.
* `WITH … DELETE/UPDATE/INSERT/MERGE` and data-modifying CTEs are writes.
* Unknown statements (`CALL`, `VACUUM`, vendor commands) count as writes.
* Key-value console commands are reads only if they are known read-only commands
  (`GET`, `HGETALL`, `SCAN`, `CONFIG GET`, `CLIENT LIST`, …); anything else is a write.
* MongoDB commands, search requests and Cypher queries use their own classifiers.

A script is assessed as a whole before anything runs: one write in a read-only script blocks
the entire script, and a `confirm writes` prompt lists every write it contains. The check
runs again when a confirmed action actually starts, for example after connecting.

### Risky operations

These are flagged with `!` in confirmations and always prompt on unrestricted
staging/production sources (unless `confirm_destructive` is off):

* `UPDATE`/`DELETE` without an effective top-level `WHERE` clause. A `WHERE` that only
  appears in a subquery does not count, and neither do trivially true predicates such as
  `WHERE 1 = 1`, `WHERE 'a' = 'a'` or `WHERE TRUE`;
* `DROP`, `TRUNCATE` and `ALTER … DROP`;
* `FLUSHALL`, `FLUSHDB`, `DEL`, `UNLINK`, `SWAPDB`, `SHUTDOWN` and `… FLUSH` subcommands;
* MongoDB `drop`/`dropDatabase`/`dropIndexes` and unfiltered updates/deletes, search
  `DELETE` requests and `_delete_by_query`/`_update_by_query`, Cypher `DETACH DELETE`.

Every confirmation starts with the exact target (data source name, configured database,
environment) and the policy, so you can tell which connection a prompt is about.

## Database-enforced read-only

Parsing SQL is not a security boundary. For read-only sources RELIX also asks the database
to refuse writes on every connection it opens (editor session and data connections):

| Engine | Mechanism |
|---|---|
| PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB | `default_transaction_read_only=on` as a startup option |
| MySQL, MariaDB | `SET SESSION TRANSACTION READ ONLY`, re-applied after every pool reset |
| SQLite | file opened read-only, plus `PRAGMA query_only` (covers `:memory:` and attached files) |

Other engines (including TiDB, which rejects the statement unless noop functions are
enabled, Redis/Valkey and all cloud engines) are only checked by RELIX. The data source
details then say `read-only (checked by RELIX only)` instead of `enforced by the database`.

A deliberate statement can still lift the session setting, for example
`BEGIN READ WRITE` or `SET default_transaction_read_only = off` on PostgreSQL. For a hard
guarantee, connect with a database account that only has read privileges.

Changing the policy of a connected data source reconnects it so the session settings match.

## Limits

* RELIX does not estimate affected rows or offer dry runs yet; prompts never show estimates.
* Writes are never retried automatically after a connection loss.
* Table grids of read-only sources open without editing support.
