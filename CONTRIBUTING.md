# Contributing to RELIX

Thanks for helping! This document explains how to set up a development environment, the
checks every change has to pass and the conventions used in the code base.

## Development environment

* Rust **1.94.1 or newer** (the repository pins the stable toolchain via
  `rust-toolchain.toml`; `rustup` installs it automatically together with `rustfmt` and
  `clippy`).
* A C compiler (SQLite and libdbus are built from bundled sources).
* Docker with Compose for the integration tests.
* A terminal with truecolor support is recommended but not required.

```bash
git clone https://github.com/t0bx/relix.git
cd relix
cargo run -- --project Playground
```

Use `RELIX_HOME=/tmp/relix-dev cargo run` to keep development data away from your real
configuration and `RELIX_LOG=debug` for verbose logs in `data/logs`.

## Formatting and linting

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Clippy warnings fail CI. Do not silence lints with `allow` attributes unless there is a
documented reason; fix the cause instead. `unsafe` code is forbidden workspace-wide.

## Tests

```bash
cargo test --workspace --all-features                     # unit, workflow and rendering tests

docker compose -f docker-compose.test.yml up -d --wait    # PostgreSQL, MySQL, MariaDB, Redis
RELIX_INTEGRATION=1 cargo test --workspace --all-features
docker compose -f docker-compose.test.yml down -v
```

* Business logic lives outside the UI and is tested without a terminal (`relix-app` has
  workflow tests that drive the real application with keystrokes against SQLite and an
  in-memory key-value driver).
* UI behaviour is tested with Ratatui's `TestBackend` in `crates/relix-tui/tests`; assert on
  content and structure, not on exact colours.
* Driver integration tests skip themselves unless `RELIX_INTEGRATION=1` is set.

CI additionally builds release binaries on Linux, Windows and macOS, builds the
documentation with warnings as errors, checks the minimum supported Rust version and runs
`cargo audit`.

## Coding conventions

* **Crate boundaries matter.** UI code (`relix-tui`) renders state and forwards input; it
  never talks to databases. Application behaviour belongs in `relix-app`, persistence in
  `relix-project`/`relix-config`, engine specifics in driver crates. See
  [ARCHITECTURE.md](ARCHITECTURE.md).
* **Small, focused files and functions** with meaningful names; one purpose per module.
* **Avoid duplication** — shared logic goes to the crate that owns the concept (for example
  SQL lexing in `relix-query`, SQL generation in `relix-driver-api::SqlBuilder`).
* **Every Rust file starts with the project header**:

  ```rust
  /*
   * @author <your name>
   * @since dd.MM.yyyy
   */
  ```

* **Comments only where they explain non-obvious behaviour** (typically a doc comment on a
  complex function). No banner or section comments.
* Typed errors (`thiserror`) in libraries, `anyhow` only at the executable boundary. No
  `unwrap()`/`expect()` in production paths; recoverable problems must never panic.
* Never block the async runtime or the render path; use `spawn_blocking` for blocking
  libraries and report results through `AppEvent`s.
* Never log or display secrets.

## Adding a database driver

Follow [docs/drivers/writing-a-driver.md](docs/drivers/writing-a-driver.md) for native
drivers and [docs/drivers/external-connectors.md](docs/drivers/external-connectors.md) for
connectors that are installed through the driver manager.

## Commits and pull requests

* Use [Conventional Commits](https://www.conventionalcommits.org/):
  `<type>[optional scope]: <description>`, e.g. `feat(driver-mysql): support generated columns`
  or `fix(tui): keep cursor visible after resize`. Split unrelated changes into separate
  commits per area.
* Keep pull requests focused, describe the user-visible change and how you tested it, and
  update documentation (README, ARCHITECTURE, docs/) when behaviour changes.
* All CI checks must pass; do not disable or weaken checks to get a green build.
