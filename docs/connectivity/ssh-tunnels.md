# SSH tunnels

SSH tunnelling is optional. A data source uses a tunnel only when its **Network** mode is
`SSH tunnel` or `VPN + SSH tunnel`; everything else connects directly.

```text
Relix ──SSH──▶ Bastion (bastion.example.com:22) ──private network──▶ PostgreSQL 10.0.0.15:5432
```

Relix opens the SSH session, listens on a local port (`127.0.0.1:<auto>` by default),
forwards every connection made to it through an SSH `direct-tcpip` channel to the database
host, and connects the unmodified database driver to that local endpoint. Drivers contain
no SSH code.

The implementation uses [russh](https://github.com/warp-tech/russh), a pure Rust SSH
implementation (ring crypto backend); no `ssh` binary is required.

## Authentication

| Method | Details |
|---|---|
| Password | Asked when needed, or stored in the OS credential store when *Save secret* is on |
| Private key | OpenSSH, PEM/PKCS#8 and PuTTY (`.ppk`) keys; Ed25519, ECDSA and RSA (`rsa-sha2-256/512` chosen automatically) |
| Encrypted private key | The passphrase is asked when needed (or stored, as above) |
| SSH agent | `SSH_AUTH_SOCK` on Linux/macOS; on Windows the OpenSSH agent pipe, then Pageant |

Missing secrets open a masked prompt; a rejected password is asked again. Secrets typed in a
prompt are kept for the running session.

**Use ~/.ssh/config** reads `HostName`, `Port`, `User` and `IdentityFile` for the configured
host alias from `~/.ssh/config` (`Host` patterns with `*`, `?` and `!`; first match wins,
like OpenSSH). Values entered in the dialog take precedence. `Match` blocks and
`ProxyJump`/`ProxyCommand` are not evaluated — use the *Jump host* field instead.

## Host key verification

Host keys are always verified; verification cannot be disabled.

* Keys are looked up in `~/.ssh/known_hosts` and in Relix's own file
  `<config>/ssh/known_hosts` (hashed entries are supported). A configuration can name its own
  known hosts file instead.
* **Ask on first use** (default): an unknown key opens a dialog with the key type and SHA-256
  fingerprint. Only after you confirm is the key written to Relix's known hosts file (or the
  configuration's own file) and the connection retried.
* **Known hosts only**: unknown keys are refused; add the key yourself after verifying it.
* A key that **differs** from a recorded one is always refused with
  `HOST KEY MISMATCH … line N` — possibly a man-in-the-middle attack. Remove the stale line
  only after confirming the server's new key with its administrator.

## Jump hosts

Set **Jump host** to another SSH configuration to reach a server only accessible through a
gateway. Chains can have up to 8 hops; each hop verifies its own host key and uses its own
credentials, and only the first hop must be reachable from your machine. Loops are detected.

```text
Relix ─▶ Gateway A ─▶ Gateway B ─▶ internal database
```

## Shared sessions and lifecycle

* All data sources that use the same SSH configuration share **one** SSH session, and data
  sources with the same target also share one local forward.
* Sessions and forwards are reference counted: disconnecting one data source never closes a
  session another one still uses. The session closes when its last connection closes.
* Keepalives are sent every *Keepalive* seconds (0 disables them); three unanswered
  keepalives mark the session as lost.
* A health monitor checks sessions every second. With *Reconnect* enabled, a lost session
  is re-established on the same local port with up to *Attempts* tries (*delay* between
  them). Only network failures are retried — authentication and host key problems never are.
  Database connections that were running over the dead session fail and reconnect on your
  next action; statements are never re-run automatically.
* Editing a configuration takes effect for new connections; running connections keep their
  session until they reconnect.

## Local endpoint

The forward binds to the configuration's **Bind address** (`127.0.0.1` by default) and a
free port. Set **Local port** on the data source to use a fixed port; if it is taken the
connection fails with *local address … is unavailable*.

## TLS through a tunnel

Drivers connect to the local endpoint but verify certificates against the **original**
database host name, so `verify-full` keeps working for PostgreSQL, MySQL/MariaDB/TiDB and the
HTTP-based engines (ClickHouse, Trino, InfluxDB, Elasticsearch/OpenSearch). The Redis client
cannot verify against a different name than it connects to, so Redis with `verify-full`
through a tunnel is refused with an explanation; use `require` (encrypted, unverified) over
the tunnel or reach Redis through a VPN.

## Which engines can be tunnelled

| Engine type | SSH tunnel |
|---|---|
| Network databases (PostgreSQL family, MySQL family, SQL Server, Oracle, ClickHouse, Trino, MongoDB, Cassandra/ScyllaDB, Neo4j, InfluxDB, Elasticsearch/OpenSearch, Redis/Valkey) | yes |
| File databases (SQLite, DuckDB) | not applicable |
| Cloud APIs that use several provider endpoints (BigQuery, DynamoDB) | no — use a VPN or the system proxy |
