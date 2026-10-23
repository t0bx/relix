# RELIX architecture

RELIX is a Cargo workspace. Every crate owns one responsibility, dependencies only point
"downwards" and the terminal UI never talks to a database connection directly: it renders
application state and dispatches input to the application layer, which coordinates
asynchronous services.

## Workspace structure

| Crate | Responsibility |
|---|---|
| `relix` | Executable: CLI, logging, settings loading, composition of drivers / driver manager / keyring, runtime and terminal startup and shutdown |
| `relix-core` | Domain model shared by everything: values, result batches, metadata, table queries, change sets, key-value model, data source definitions, dialects, ids |
| `relix-config` | Platform paths, global settings with validation/migration/recovery, key chord parsing, colours and themes, atomic owner-only file writes |
| `relix-project` | Projects, data source associations, sessions (layout, tabs, drafts), scripts, query/filter history persistence, credential storage (OS keyring with in-memory fallback), project export/import |
| `relix-driver-api` | Driver interfaces (`DatabaseDriver`, `DatabaseConnection`, `SqlIntrospector`, `SqlExecutor`, `TableDataAccess`, `KeyValueConnection`), capabilities, typed errors, timeouts, dialect-aware SQL builder, external connector IPC protocol |
| `relix-driver-postgres` | PostgreSQL adapter (`tokio-postgres`, rustls) |
| `relix-driver-mysql` | MySQL and MariaDB adapters sharing one implementation (`mysql_async`, rustls) |
| `relix-driver-sqlite` | SQLite adapter (`rusqlite`, bundled SQLite) |
| `relix-driver-redis` | Redis adapter (`redis`, rustls) implementing the key-value interface |
| `relix-driver-manager` | Driver registry, package registries, signed downloads, atomic installation, external connector runtime |
| `relix-connector-sdk` | Library for writing external connectors plus the reference `relix-csv-connector` |
| `relix-query` | SQL lexer, statement splitter, statement classification, script execution coordination (cancellation, timeouts), history, export, result wrapping for server-side paging |
| `relix-editor` | Text buffer with undo/redo, movement, search/replace, highlighting, diagnostics, completion engine |
| `relix-app` | Application state and behaviour: commands and keymap, workspace (panes/tabs), explorer, editor/table/key-value tabs, data grid, overlays, connection lifecycle, background tasks, session save/restore |
| `relix-tui` | Ratatui rendering, crossterm input mapping, terminal lifecycle, event loop, colour handling |
| `relix-testing` | Integration test configuration (docker databases) and an in-memory key-value mock driver |

## Crate dependencies

```text
relix ─┬─> relix-tui ──────> relix-app ─┬─> relix-editor ──> relix-query ──> relix-driver-api ──> relix-core
       │                                ├─> relix-query
       │                                ├─> relix-project ──> relix-config
       │                                └─> relix-driver-api
       ├─> relix-driver-manager ──> relix-driver-api, relix-config
       ├─> relix-driver-postgres ┐
       ├─> relix-driver-mysql    ├─> relix-driver-api, relix-query (shared SQL lexer)
       ├─> relix-driver-sqlite   ┘
       └─> relix-driver-redis ──> relix-driver-api
relix-connector-sdk ──> relix-driver-api, relix-driver-sqlite (reference connector)
relix-testing ──> relix-driver-api
```

`relix-app` does not know any concrete driver. It talks to drivers through the
`DriverService` trait (implemented in the executable on top of `DriverManager`, and by
`StaticDriverService` in tests). There are no dependency cycles; `relix-app` only depends on
`relix-driver-sqlite`/`relix-testing` through its optional `test-support` feature.

## Data flow and application event loop

```text
 crossterm events ─┐
                   ├─> relix-tui event loop ──InputEvent──> App::handle_input ──> Command / text input
 tick (250 ms) ────┤                                                │
                   │                                                ├─> state changes (pure, synchronous)
 AppEvent channel ─┘<─────────── background tasks <─── App::spawn ──┘   (connect, load, execute, commit…)
        ^                                                                       │
        └──────────────────────── bounded mpsc (1024) <──────────────────────────┘
```

1. `relix-tui` converts terminal events to terminal-independent `KeyChord`s and mouse
   events and calls `App::handle_input`.
2. The app resolves keys through the `Keymap`: the focused region yields a list of key
   contexts (e.g. `Grid → Navigable → NonText → Global`); the first context that knows the
   sequence wins, prefixes stay pending (which-key). In text inputs printable keys are
   inserted instead of being looked up, which resolves conflicts such as `j`.
3. Commands mutate state synchronously. Anything that touches a database is spawned on
   the Tokio runtime and reports back with an `AppEvent` through a bounded channel, which
   applies back-pressure instead of growing memory.
4. Every request carries a `RequestId`; results for superseded requests (a newer page, a
   closed tab) are dropped.
5. The event loop redraws only after input, results or the tick changed something, and
   coalesces bursts of results into one frame.

Rendering never blocks: it only reads state (and records click targets and jump-label
positions for the next input).

## Driver abstraction

```text
DatabaseDriver ── metadata(), capabilities(), connect(config) ──> Box<dyn DatabaseConnection>
DatabaseConnection ── server_info, ping, close, as_sql(), as_key_value()
SqlConnection = SqlIntrospector + SqlExecutor + TableDataAccess
KeyValueConnection ── scan_keys, describe_key, read_value, write, execute_command, server_details
```

* Relational engines expose databases → schemas → objects → table details; capabilities say
  whether databases and schemas exist (MySQL has no schemas, SQLite uses attached
  databases as "databases").
* `SqlExecutor::execute` runs exactly one statement on a long-lived session connection,
  so user transactions (`BEGIN … COMMIT`) keep their meaning. Result sets are streamed and
  truncated at a row limit without buffering the rest.
* Table pages, counts and change sets use separate connections or pools where the engine
  allows it, so long editor queries do not block browsing.
* `SqlBuilder` generates page queries (`LIMIT n+1` to detect further pages, stable
  tie-breaker ordering) and change statements for every dialect. Updates and deletes are
  keyed by the row identity and guarded by original values; drivers apply a change set in
  one transaction and roll back unless every statement affects exactly one row.
* Redis is not forced into SQL: it has its own interface, SCAN based discovery and paged
  value reads.

Errors are typed (`DriverError` with an `ErrorKind`, detail and position) so the UI can
distinguish authentication failures (prompt for a password), lost connections (mark the
source disconnected, never retry a statement whose outcome is unknown), conflicts and
syntax errors (underline the position in the editor).

## Project persistence

```text
<config>/config.toml                  global settings (validated, migrated, quarantined if corrupt)
<config>/themes/*.toml                custom themes
<data>/recent.toml                    recently opened projects
<data>/projects/<id>/project.toml     project metadata and data source definitions (no secrets)
<data>/projects/<id>/session.json     panes, tabs, cursor positions, connected sources, explorer state
<data>/projects/<id>/drafts/*.sql     unsaved editor contents
<data>/projects/<id>/scripts/*.sql    saved scripts
<data>/projects/<id>/history.jsonl    query history (append-only, newest N loaded)
<data>/projects/<id>/filters.json     WHERE filter history per table
<data>/drivers/installed.toml         installed external driver packages
<data>/logs/relix.log.*               daily rotated logs
OS keyring: service "relix", account "project/<id>/datasource/<id>"
```

All files are written atomically (temporary file + rename) with owner-only permissions on
Unix. Sessions are saved on exit, on project switch and every 30 seconds.

## UI component architecture

* `relix-app` holds all UI state: `Workspace<Tab>` (a binary split tree of panes, each with
  tabs), `ExplorerState`, overlays (pickers, forms, dialogs, editors, jump labels, help),
  notifications and the keymap. Tabs are `EditorTab`, `TableTab` and `KeyValueTab`; grids are
  `GridState` plus `ResultQuery` (sort, filter, paging) so table panes stay independent.
* `relix-tui/src/ui` renders it: `header`, `sidebar`, `panes` (tab bars), `editor`, `grid`,
  `table`, `keyvalue`, `status` (mode, hints), `toasts` and `overlays/*`.
* Themes are `ThemePalette<ColorSpec>` in `relix-config`, mapped to ratatui colours once per
  theme change, with a 256-colour fallback when the terminal does not advertise truecolor.

## Async task management

* One multi-threaded Tokio runtime is created by the executable; `App` spawns tasks through
  its runtime handle.
* Long operations are cancellable: script execution uses a `CancellationToken`; cancelling
  or hitting the configured timeout asks the driver to cancel server-side (PostgreSQL cancel requests,
  `KILL QUERY`, SQLite interrupt) and waits briefly for the statement to stop.
* Connection attempts are bounded by the connect timeout; actions that need a connection
  are deferred and resumed once it is established.
* Blocking libraries (SQLite, the OS keyring) run on the blocking thread pool.

## Driver installation mechanism

Missing drivers are resolved through the `DriverManager`:

1. a data source references a driver id that is not registered;
2. configured HTTPS registries are queried for a release compatible with the current OS,
   CPU architecture and driver API version;
3. the user sees name, version, source and size and approves the installation;
4. the package is downloaded over HTTPS, its size and SHA-256 are checked and the ed25519
   signature of `relix-package-v1 / id / version / api / target / size / sha256` is
   verified with the registry's public key;
5. the archive is extracted into a staging directory (no absolute paths, `..`, links or
   devices), its `connector.toml` validated, and the directory atomically renamed into
   place; the previous version stays until the new one is registered;
6. the external connector is registered and the connection is retried.

External connectors run as separate processes and speak a versioned JSON-lines protocol
over stdin/stdout. The full specification, package and index formats, signing procedure
and trust model are in [docs/drivers/external-connectors.md](docs/drivers/external-connectors.md).
The five MVP engines are compiled in; they are not, and cannot be, dynamically loaded.
