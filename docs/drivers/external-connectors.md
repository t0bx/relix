# External connectors

RELIX ships five built-in engines (PostgreSQL, MySQL, MariaDB, SQLite, Redis) compiled into
the binary. Everything else is an **external connector**: a separate executable, installed
from a signed package, that RELIX starts as a child process and talks to over a small
JSON-lines protocol. Rust crates cannot be loaded at runtime, so this process boundary is
what makes optional drivers downloadable.

Crates involved:

| Crate | Role |
| --- | --- |
| `relix-driver-api` (`ipc` module) | Wire types of the protocol (`IpcRequest`, `IpcCall`, `IpcResponse`, `HandshakeResult`, `IpcConnectionConfig`, `IPC_PROTOCOL_VERSION`). |
| `relix-driver-manager` | Driver registry, package index, download, verification, installation, `ExternalDriver` (the RELIX side of the protocol). |
| `relix-connector-sdk` | `serve(driver)` for connector authors, plus the reference connector `relix-csv-connector`. |

## Lifecycle when a data source needs a driver

1. Determine the driver id from the data source (`DataSourceDefinition::driver`).
2. `DriverManager::driver(id)` — built-in or installed and compatible? Connect.
3. Otherwise `DriverManager::offer_for(id)` (after `refresh_index()`) returns the newest
   compatible package: release name, version, registry name and download size.
4. The UI shows the offer and asks the user. Nothing is downloaded or executed before that.
5. `DriverManager::install(offer, progress)` downloads over HTTPS, verifies size, SHA-256 and
   the ed25519 signature, unpacks into a staging directory, validates the manifest and
   atomically moves it to `<drivers_dir>/<id>/<version>`, records it in `installed.toml`
   with `approved = true` and registers it.
6. Retry the connection with the newly registered driver.

`entries()` lists all drivers with status `Ready`, `UpdateAvailable(version)` or
`Incompatible(reason)`; `outdated()` lists installed drivers with a newer compatible
release; installing that newer offer is the update (older versions are removed afterwards);
`remove(id)` deletes an external driver. Built-in drivers can be neither installed over nor
removed.

In **offline mode** (`[drivers] offline = true` in the settings, or `set_offline(true)`),
`refresh_index()` and `install()` fail with `ManagerError::Offline`; installed drivers keep
working.

## IPC protocol (version 1)

The connector reads requests from **stdin** and writes responses to **stdout**, one UTF-8
JSON document per line (`\n` terminated). stdout is reserved for the protocol; connectors
must log to **stderr** (RELIX keeps the last 8 KiB and shows it when the process dies).

Request:

```json
{"id": 7, "method": "fetch_table_page", "params": {"table": {...}, "query": {...}}}
```

Response, either

```json
{"id": 7, "result": {...}}
```

or

```json
{"id": 7, "error": {"kind": "query", "message": "no such column: x", "detail": null, "position": 12}}
```

`id` is chosen by RELIX and unique per process. Several requests can be in flight at once
(for example `cancel` while `execute` is still running) and responses may be written in any
order; RELIX matches them by `id`. Methods without parameters still send `"params": {}`.
`error.kind` is one of the `ErrorKind` values in snake case (`connection`, `authentication`,
`tls`, `timeout`, `cancelled`, `query`, `concurrent_modification`, `ambiguous_row`,
`unsupported`, `protocol`, `io`, `configuration`, `internal`).

| Method | Params | Result |
| --- | --- | --- |
| `handshake` | `{protocol_version}` | `{protocol_version, api_version, metadata: DriverMetadata, capabilities: DriverCapabilities}` |
| `connect` | `{settings: ConnectionSettings, password: string\|null}` | `ServerInfo` |
| `ping` | `{}` | `null` |
| `close` | `{}` | `null` |
| `server_info` | `{}` | `ServerInfo` |
| `databases` | `{}` | `[DatabaseInfo]` |
| `schemas` | `{database}` | `[string]` |
| `objects` | `{database, schema}` | `[ObjectInfo]` |
| `table_details` | `{table: TableRef}` | `TableDetails` |
| `object_definition` | `{object: ObjectInfo}` | `string\|null` |
| `execute` | `{statement, options: {row_limit}}` | `StatementOutcome` |
| `cancel` | `{}` | `null` |
| `fetch_table_page` | `{table, query: TableQuery}` | `RowBatch` |
| `count_rows` | `{table, filter}` | `number` |
| `apply_changes` | `{changes: ChangeSet}` | `ChangeReport` |

All payload types are the serde representations of the `relix-core` / `relix-driver-api`
types of the same name; the semantics are those of `SqlIntrospector`, `SqlExecutor` and
`TableDataAccess` (one statement per `execute`, at most `row_limit` rows with `has_more`,
`apply_changes` in a single transaction, ...).

Session rules:

- RELIX starts **one process per connection**. The first request is `handshake`, then
  `connect`. A second `connect` in the same process is an error.
- RELIX rejects the connector unless `protocol_version == 1`, `api_version` equals its
  `DRIVER_API_VERSION`, `capabilities.data_model == "relational"` and `metadata.id` matches
  the installed package id.
- `close` closes the database connection (interrupting running statements where possible)
  and answers `null`; afterwards RELIX closes stdin. A process that does not exit within two
  seconds is killed. If the process exits unexpectedly, every outstanding request fails with
  a `connection` error.
- The password travels in clear text over the private stdin pipe only; connectors must never
  log it.

### Writing a connector

Implement the regular `relix_driver_api::DatabaseDriver` trait (exactly like a built-in
driver) and hand it to the SDK:

```rust
#[tokio::main]
async fn main() -> std::process::ExitCode {
    match relix_connector_sdk::serve(std::sync::Arc::new(MyDriver::new())).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("my-connector: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
```

`serve` handles the handshake, keeps the connection, runs every request on its own task and
serializes the output. `relix-connector-sdk/src/bin/relix-csv-connector` is a complete
example: it imports a directory of CSV files (path from `file_path`, or `database`) into an
in-memory SQLite database through the built-in SQLite driver and exposes them as read-only
tables.

During development `ExternalDriver::probe(path_to_executable)` starts a connector, reads its
handshake and returns a usable driver without packaging it.

## Package format

A package is a `.tar.gz` archive containing a manifest and the executable:

```
connector.toml
bin/relix-csv-connector
```

`connector.toml`:

```toml
id = "csv"                       # driver id, must equal the index entry
name = "CSV Files"               # display name
version = "1.0.0"                # semver, must equal the index entry
api_version = 1                  # RELIX driver API version, must equal the index entry
executable = "bin/relix-csv-connector"   # relative path inside the package
dialect = "sqlite"               # optional: postgres | my_sql | sqlite | generic

[target]                         # how data sources address the database
kind = "file"                    # or: kind = "network", default_port = 5432

[capabilities]                   # optional, defaults to DriverCapabilities::relational()
data_model = "relational"
multiple_databases = false
schemas = false
transactions = false
cancellation = true
table_editing = false
routines = false
tls = false
```

The installer only accepts regular files and directories with relative paths (no absolute
paths, `..`, symlinks, hard links or devices), caps the unpacked size at 1 GiB and ignores
file modes from the archive; the executable gets mode `0755` on Unix. The manifest metadata
is what RELIX shows in the driver list, so installed packages are never started just to be
listed.

## Index format

Each registry serves one JSON document over HTTPS:

```json
{
  "format_version": 1,
  "releases": [
    {
      "id": "csv",
      "name": "CSV Files",
      "version": "1.0.0",
      "api_version": 1,
      "description": "Directory of CSV files as read-only SQL tables",
      "artifacts": [
        {
          "target": "linux-x86_64",
          "url": "https://drivers.example.org/csv/1.0.0/linux-x86_64.tar.gz",
          "size": 1834221,
          "sha256": "9f2c…64 hex chars…",
          "signature": "base64 of the 64-byte ed25519 signature"
        }
      ]
    }
  ]
}
```

`target` is `<os>-<arch>` as reported by Rust's `std::env::consts`, e.g. `linux-x86_64`,
`linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`. A release is offered only
when its `api_version` matches, an artifact for the current target exists, the artifact URL
is `https://` and its size is at most 256 MiB. Among matching releases the highest semver
version wins; on equal versions the registry listed first in the settings wins. The index
itself is limited to 8 MiB.

Registries are configured in `config.toml`:

```toml
[drivers]
offline = false

[[drivers.registries]]
name = "official"
url = "https://drivers.example.org/index.json"
public_key = "base64 of the 32-byte raw ed25519 public key"
```

## Signing procedure

For every artifact the registry signs this exact UTF-8 message (fields separated by `\n`,
no trailing newline, `sha256` in lower-case hex):

```
relix-package-v1
<id>
<version>
<api_version>
<target>
<size>
<sha256>
```

Both the signature (64 bytes) and the public key (32 raw bytes) are encoded with standard
base64 including padding. Example with OpenSSL 3 (on the registry operator's machine):

```sh
openssl genpkey -algorithm ed25519 -out registry.pem
openssl pkey -in registry.pem -pubout -outform DER | tail -c 32 | base64   # public_key

sha=$(sha256sum csv-linux-x86_64.tar.gz | cut -d' ' -f1)
size=$(stat -c %s csv-linux-x86_64.tar.gz)
printf 'relix-package-v1\ncsv\n1.0.0\n1\nlinux-x86_64\n%s\n%s' "$size" "$sha" > message
openssl pkeyutl -sign -inkey registry.pem -rawin -in message | base64 -w0   # signature
```

`relix_driver_manager::signing_message` produces the same message for tooling written in
Rust.

## Trust model

- **Trust anchor:** the registry's ed25519 public key in the user's configuration. The index
  is not trusted by itself; every artifact must carry a valid signature by that key over its
  id, version, API version, target, size and SHA-256. A tampered index can hide releases but
  cannot make RELIX install bytes the registry did not sign, nor relabel a signed package
  for another version, target or API version.
- **Transport:** HTTPS only (rustls with the ring provider and the operating system's trust
  store), redirects included; `http://`, `file://` and other schemes are refused for both
  indexes and artifacts. Transport security is defence in depth; integrity comes from the
  signature.
- **Integrity:** the download is limited to the advertised size, then the exact byte length,
  the SHA-256 and the signature are checked before anything is written to the drivers
  directory.
- **Consent:** a downloaded executable is only ever started for a package the user approved
  (`install()` is the approval and records `approved = true`). Records without approval,
  for another target or for another API version are reported as incompatible and never
  started.
- **Atomicity:** packages are unpacked into a staging directory inside the drivers directory
  and renamed into place; `installed.toml` is written atomically. Any failure removes the
  staging directory and leaves the previous installation untouched. Leftovers from crashes
  are cleaned on the next `load_installed()`.
- **Isolation:** a connector runs as a separate process with the user's privileges. It is
  not sandboxed: approving a package means trusting its publisher like any other program
  the user installs.

## Limitations

- Protocol version 1 supports **relational (SQL) connectors only**. Key-value connectors
  (the `KeyValueConnection` surface) cannot be external yet.
- One connector process per connection; there is no connection pooling across processes.
- Responses are single JSON lines (max 256 MiB); there is no streaming of large results,
  so connectors must honour `row_limit`.
- Packages are not handshaken at install time; a connector whose binary does not work on
  the machine is detected when the first connection is opened.
- Signing keys cannot be rotated per release; a key change requires updating the registry
  entry in the settings.
- The reference CSV connector loads all files into memory on connect and infers column types
  (INTEGER, REAL, TEXT) from the data.
