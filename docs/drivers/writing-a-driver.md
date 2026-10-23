# Writing a database driver

RELIX supports two kinds of drivers:

* **Native drivers** are Rust crates compiled into the `relix` executable. The five MVP
  engines are native drivers. Use this for engines that should ship with RELIX.
* **External connectors** are separate executables installed through the driver manager
  and spoken to over a JSON-lines protocol. Use this for engines that are distributed
  independently — see [external-connectors.md](external-connectors.md).

Both implement the same traits from `relix-driver-api`, so the application, explorer,
editor and grid work the same for every engine.

## 1. Create the crate

```text
crates/relix-driver-exampledb/
├── Cargo.toml
├── src/
│   ├── lib.rs            # pub use driver::ExampleDriver;
│   ├── driver.rs         # DatabaseDriver: metadata, capabilities, connect
│   ├── connection.rs     # DatabaseConnection
│   ├── introspection.rs  # SqlIntrospector
│   ├── execution.rs      # SqlExecutor
│   ├── table_data.rs     # TableDataAccess
│   ├── values.rs         # engine values -> relix_core::Value
│   └── errors.rs         # engine errors -> DriverError
└── tests/integration.rs
```

`Cargo.toml` inherits the workspace settings:

```toml
[package]
name = "relix-driver-exampledb"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
relix-core.workspace = true
relix-driver-api.workspace = true
async-trait.workspace = true
tokio.workspace = true

[lints]
workspace = true
```

TLS must use rustls with the `ring` provider; OpenSSL, native-tls and aws-lc are not
allowed because releases are built for five targets.

## 2. Describe the driver

```rust
pub struct ExampleDriver;

#[async_trait]
impl DatabaseDriver for ExampleDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            "exampledb",
            "ExampleDB",
            Some(Dialect::Generic),
            ConnectionTarget::Network { default_port: 7000 },
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            multiple_databases: true,
            schemas: true,
            cancellation: true,
            tls: true,
            ..DriverCapabilities::relational()
        }
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let timeout = config.settings.connect_timeout();
        with_timeout(timeout, open(config)).await.map(|connection| Box::new(connection) as _)
    }
}
```

Capabilities drive the UI: `schemas = false` skips the schema level in the explorer,
`table_editing = false` keeps grids read-only, `data_model = KeyValue` (see
`DriverCapabilities::key_value()`) switches to the key browser and console.

## 3. Implement the connection

* `DatabaseConnection::as_sql()` returns `Some(self)` for relational engines,
  `as_key_value()` for key-value stores.
* `SqlIntrospector` returns databases, schemas, objects (tables, views, routines,
  sequences, triggers) and `TableDetails` (columns, primary key, indexes, foreign keys and
  a DDL/definition string). It is called lazily per explorer level.
* `SqlExecutor::execute` runs **one** statement on a long-lived session connection so
  `BEGIN`/`COMMIT` issued by the user keep their meaning. Stream rows, stop storing them
  after `options.row_limit`, set `has_more` and drain the rest. Return
  `StatementOutcome::Rows` for statements with a result set (even when empty),
  `Affected { rows }` for DML and `Completed { message }` otherwise. `cancel` must not wait
  for the running statement (use a cancel token or a second connection).
* `TableDataAccess` builds its SQL with `SqlBuilder` (pick the dialect and the parameter
  style your client supports): `select_page` already requests one extra row to detect
  further pages; `apply_changes` must run every change in one transaction and roll back
  unless each update/delete affects exactly one row (`ErrorKind::ConcurrentModification`).
* Use separate connections or a pool for introspection and table pages so a long editor
  query does not block browsing.

## 4. Map errors and values

Convert engine errors into `DriverError` with the matching `ErrorKind`. The UI relies on
it: `Authentication` prompts for a password, `Connection`/`Io` mark the data source as
disconnected, `Cancelled`/`Timeout` are reported as such, `position` (0-based characters in
the statement) underlines the error in the editor, `detail` carries hints. Never include
passwords in messages.

Convert values to `relix_core::Value` (`Null`, `Bool`, `Int`, `UInt`, `Float`, `Text`,
`Bytes`). Keep exact decimals, dates and engine specific types as `Text` so they round-trip
when edited.

## 5. Register the driver

Add the crate to `crates/relix/Cargo.toml` and register it in
`crates/relix/src/startup.rs`:

```rust
manager.register_built_in(Arc::new(relix_driver_exampledb::ExampleDriver));
```

Optionally give it a position in the engine list in `crates/relix/src/driver_service.rs`.

## 6. Test it

* Unit-test pure helpers (value conversion, error mapping, statement classification).
* Add integration tests that run against a real server. Add the server to
  `docker-compose.test.yml`, expose its settings through
  `relix_testing::integration::integration_config`, and skip when `RELIX_INTEGRATION` is
  not set. Cover connecting, authentication errors, unreachable servers, introspection,
  execution, paging (`has_more`), insert/update/delete through `apply_changes`, commit and
  rollback on a conflicting change.
* Use `relix_testing::integration::unique_name` for objects so test runs do not collide.

Then run the full check suite from [CONTRIBUTING.md](../../CONTRIBUTING.md).
