# RELIX

**A modern database IDE for your terminal.**

RELIX is a keyboard-driven database development environment built with Rust and
[Ratatui](https://ratatui.rs). It combines a lazy database explorer, a real SQL editor with
metadata-aware completion, an editable data grid with transactional commits, a dedicated
Redis browser and a project/workspace model similar to JetBrains DataGrip — without leaving
the terminal.

```text
 RELIX  │ Project: Development │ PostgreSQL: Main PG │ ● Connected
╭ DATA SOURCES ─────────────────╮╭ public.customers — Main PG ─────────────────────────────╮
│ ▾ ● Main PG PostgreSQL        ││ ▦ customers*                                            │
│   ▾ ◫ relix_test default      ││ WHERE tier = 'gold'                     ORDER BY name ▼ │
│     ▾ ◇ public                ││  #  ⚷id   name ▼       email            tier            │
│       ▾ ▤ tables 1            ││ *1     99 VIP customer c99@example.com  gold            │
│         ▸ ▦ customers         ││  2     96 customer 96  c96@example.com  gold            │
│       ▸ ▤ sequences 1         ││  3     93 customer 93  c93@example.com  gold            │
│ ▸ ● Cache Redis               ││ rows 1-200 of ? · 1 pending change(s) — Ctrl+S review    │
╰───────────────────────────────╯╰─────────────────────────────────────────────────────────╯
 Enter edit  Ins insert  Del delete  w where  s sort  S multi-sort  v json  Ctrl+S commit
 GRID  public.customers · rows 1-200 of ?               Rows: 1-200 of ? │ Ctrl+P Commands
```

## Features

- **Projects & workspaces** — multiple independent projects, each with its own data sources,
  saved scripts, query history and persisted session (pane layout, tabs, unsaved drafts,
  connected sources, expanded explorer nodes). Switching projects never loses unsaved
  editor content.
- **25 native engines** — relational, analytics, key-value, document, search, wide-column,
  graph and time-series databases are compiled in and work out of the box (TLS with
  `disable` / `prefer` / `require` / `verify-full`).
- **SSH tunnels and VPN** — reusable, named SSH and VPN configurations (global or per
  project), jump hosts, shared sessions, explicit host key trust, VPN + SSH combinations.
- **Database explorer** — lazily loaded tree of databases, schemas, tables, views,
  routines, sequences, columns, indexes and foreign keys, with filtering, refresh and
  DDL/definition inspection.
- **SQL editor** — multi-line editing with syntax highlighting, line numbers, selection,
  undo/redo, search & replace, comment toggling, multiple tabs and file open/save.
  Execute the selection, the statement under the cursor or the whole script (dialect-aware
  splitting that respects strings, dollar quotes, trigger bodies and MySQL `DELIMITER`).
- **Intelligent completion** — context-aware suggestions from live metadata: schemas,
  tables, columns (resolved through aliases), routines, dialect functions, data types,
  keywords and previously executed statements. Missing column metadata is fetched in the
  background.
- **Query execution** — asynchronous with progress, per-statement results, affected rows,
  durations, server error diagnostics underlined in the editor, cancellation and timeouts,
  query history with re-run, and a `TX` indicator while a session transaction is open.
- **Data grid** — horizontal/vertical scrolling, multi-row selection, column resizing and
  hiding, NULL/binary/large-text representation, cell inspector (pretty JSON, hex dump),
  copy as TSV, server-side pagination, multi-column sorting, WHERE filters with
  completion (columns, operators, functions and real values) and per-table history,
  JSON view, CSV/JSON export and flash-jump to rows and columns.
- **Transactional editing** — edit cells inline or in a multi-line popup, insert and delete
  rows; changes are staged and highlighted, reviewed in a summary and committed in one
  transaction keyed by the primary key (or a unique `NOT NULL` index). Each update is
  guarded by the originally fetched values, so concurrent modifications roll the whole
  commit back instead of silently overwriting them. Tables without a reliable row
  identity are read-only.
- **Redis browser** — incremental `SCAN` key discovery with patterns, type/TTL/encoding/size
  inspection, paged views of strings, lists, sets, sorted sets, hashes and streams, editing
  of strings and elements, key creation/deletion, TTL management and a command console
  with history (blocking and connection-hijacking commands are refused).
- **Tiling workspace** — split panes horizontally/vertically, resize, close, move focus
  directionally, open the same table in several panes with independent state.
- **Keyboard first** — central command registry, fuzzy command palette, configurable and
  context-aware key bindings, leader key with which-key popup, contextual hint bar,
  flash-jump labels and full contextual help. Mouse support for focus, tabs and scrolling.
- **Driver manager** — lists built-in and installed drivers, finds missing drivers in signed
  package registries, shows name/version/source/size, installs only after approval,
  verifies SHA-256 and ed25519 signatures, installs atomically with rollback and runs
  connectors as separate processes. See [external connectors](docs/drivers/external-connectors.md).
- **Safety** — passwords live in the OS credential store (Keychain, Windows Credential
  Manager, Secret Service) and never in project files or logs; configuration files are
  written atomically with owner-only permissions; production/staging data sources are
  colour-coded. Per data source [execution policies](docs/execution-policies.md)
  (`read-only`, `confirm writes`, `unrestricted`) apply to the editor, grid commits and
  key-value/document writes alike; production defaults to read-only, enforced by
  PostgreSQL, MySQL/MariaDB and SQLite sessions themselves, and every prompt names the exact
  target connection.
- **Themes** — dark, light and high-contrast themes, custom theme files and per-colour
  overrides with automatic 256-colour fallback.

## Supported platforms

| Platform | Architecture | Release artifact |
|---|---|---|
| Linux | x86_64 | `relix-vX.Y.Z-linux-x86_64.tar.gz` |
| Linux | aarch64 | `relix-vX.Y.Z-linux-aarch64.tar.gz` |
| Windows | x86_64 | `relix-vX.Y.Z-windows-x86_64.zip` |
| macOS | x86_64 | `relix-vX.Y.Z-macos-x86_64.tar.gz` |
| macOS | aarch64 (Apple Silicon) | `relix-vX.Y.Z-macos-aarch64.tar.gz` |

Every artifact is built natively on GitHub Actions, smoke-tested and published with SHA-256
checksums (`SHA256SUMS`).

## Supported databases

25 engines are compiled in:

| Family | Engines |
|---|---|
| PostgreSQL protocol | PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB |
| MySQL protocol | MySQL, MariaDB, TiDB |
| Embedded | SQLite, DuckDB |
| Enterprise SQL | Microsoft SQL Server, Oracle Database (needs Oracle Instant Client) |
| Analytics & cloud | ClickHouse, Trino, Snowflake, Google BigQuery |
| Key-value | Redis, Valkey |
| Document & search | MongoDB, Amazon DynamoDB, Elasticsearch, OpenSearch |
| Wide-column, graph, time-series | Apache Cassandra, ScyllaDB, Neo4j, InfluxDB 3 |

Each engine keeps its native data model: a SQL/CQL/Cypher/PartiQL console where the engine
has one, a document browser for document and search stores, a key browser for Redis/Valkey.
Connection fields, properties, authentication and verified versions are listed in
[docs/engines](docs/engines/README.md). Further engines can be added as signed
[external connectors](docs/drivers/external-connectors.md).

## Connecting through SSH and VPN

Data sources connect directly, through an **SSH tunnel** (password, key, encrypted key or
agent; known-hosts verification; jump hosts; shared sessions; automatic reconnection),
through a **VPN** (an existing system VPN that Relix verifies, or a WireGuard/OpenVPN profile
Relix brings up with your approval) or through **VPN + SSH**. SSH servers and VPN profiles are
reusable named configurations, global or per project (`Space n`):

* [Reusable server configurations](docs/connectivity/server-configurations.md)
* [SSH tunnels](docs/connectivity/ssh-tunnels.md) · [VPN](docs/connectivity/vpn.md)
* [Examples](docs/connectivity/examples.md) · [Troubleshooting](docs/troubleshooting.md)
* [Security model](docs/security.md)

## Installation

### Arch Linux (x86_64)

```bash
curl -sLO https://raw.githubusercontent.com/t0bx/relix/main/relix.asc
sudo pacman-key --add relix.asc
sudo pacman-key --lsign-key CC94E0FAF0D2B30D39B66F5D309032DF0CF40806
```

Add to `/etc/pacman.conf`:

```ini
[relix]
SigLevel = Required
Server = https://github.com/t0bx/relix/releases/download/repo
```

Then `sudo pacman -Syu relix`. Updates arrive with regular system upgrades.

### Other platforms

Download the archive for your platform from the
[releases page](https://github.com/t0bx/relix/releases), verify it and put `relix` on your
`PATH`:

```bash
sha256sum --check SHA256SUMS --ignore-missing
tar -xzf relix-v0.1.0-linux-x86_64.tar.gz
install relix-v0.1.0-linux-x86_64/relix ~/.local/bin/
```

Or build from source (Rust 1.94.1 or newer; a C/C++ compiler is needed for the bundled DuckDB, SQLite and ODPI-C):

```bash
cargo install --locked --path crates/relix
```

On Linux the Secret Service (GNOME Keyring, KWallet, KeePassXC, …) is used for passwords.
Without one, RELIX keeps passwords for the current session only and tells you so.

## Quick start

```bash
relix                         # open the most recent project
relix --project Development   # open or create a project by name
relix --theme light           # pick a theme for this session
relix --no-mouse              # keep the terminal's own text selection
relix --print-paths           # show configuration, data and log directories
```

1. Press `Ctrl+K a` (or `Space a` outside text inputs) to add a data source and `Ctrl+T`
   in the form to test it.
2. Select it in the explorer and press `→`/`l` to connect and browse; `Enter` on a table
   opens it in the data grid.
3. Press `w` in the grid to filter (`WHERE` expression), `s`/`S` to sort, `Enter` to edit
   a cell, `Ctrl+S` to review and commit.
4. Press `Ctrl+N` for a query tab, type SQL (completion pops up automatically, or
   `Ctrl+Space`) and run it with `Ctrl+Enter` / `F5` (statement) or `F9` (script).
5. `Ctrl+P` opens the command palette, `F1` shows every shortcut for the current focus.

## Keyboard shortcuts

| Key | Action |
|---|---|
| `Ctrl+P` | Command palette |
| `Ctrl+O` | Projects (open, create `Ctrl+N`, rename `Ctrl+R`, delete `Ctrl+D`) |
| `Ctrl+N` | New query tab |
| `Ctrl+S` | Save script / review & commit pending table changes |
| `Ctrl+Enter`, `F5`, `Alt+Enter` | Execute selection or current statement |
| `Ctrl+Shift+Enter`, `F9` | Execute entire script |
| `F8`, `Ctrl+F2` | Cancel running query |
| `Ctrl+F` | Find (editor), filter (explorer, grid) |
| `Ctrl+W` | Close tab |
| `Tab` / `Shift+Tab`, `F6` | Cycle focus |
| `Alt+h/j/k/l`, `Alt+arrows` | Focus pane in a direction |
| `Alt+v` / `Alt+s` / `Alt+x` | Split vertically / horizontally / close pane |
| `Ctrl+G` | Flash-jump to a pane, tab or the explorer |
| `Ctrl+K …`, `Space …` | Leader key with which-key popup |
| `?`, `F1` | Contextual help |
| `Esc` | Dismiss popup, leave input, clear selection |
| `j` / `k` | Navigate in lists, explorer and grids |

The complete, context-sensitive list is in [docs/keybindings.md](docs/keybindings.md) and
inside the application (`F1`).

## Configuration

`relix --print-paths` shows where files live (override everything with `RELIX_HOME`).
The global configuration is `config.toml`; it is created with defaults on first start and
a broken file is backed up and replaced instead of preventing startup:

```toml
theme = "dark"

[ui]
show_hints = true
sidebar_width = 34
mouse = true

[query]
row_limit = 500          # rows fetched per statement
page_size = 200          # rows per table page
timeout_secs = 300       # 0 disables the query timeout
confirm_destructive = true

[keybindings]
"palette.open" = ["ctrl+p", "f2"]
"query.run" = ["ctrl+enter", "f10"]

[theme_overrides]
accent = "#ff8800"
```

Themes, key binding ids, driver registries and project files are documented in
[docs/configuration.md](docs/configuration.md).

## Building and testing

```bash
cargo build --workspace
cargo test --workspace --all-features

# integration tests against real servers: start the engines of the crate you test
docker compose -f docker-compose.test.yml up -d --wait mongodb
RELIX_INTEGRATION=1 cargo test -p relix-driver-mongodb

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

The architecture is described in [ARCHITECTURE.md](ARCHITECTURE.md); native build
requirements (DuckDB, Oracle Instant Client) in [docs/engines/deployment.md](docs/engines/deployment.md).

## Contributing

Contributions are welcome — please read [CONTRIBUTING.md](CONTRIBUTING.md), which also
explains how to add a new database driver.

## License

RELIX is licensed under the [MIT license](LICENSE).
