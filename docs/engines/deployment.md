# Building, deployment and licensing

Every engine is a separate crate (`crates/relix-driver-*`) linked into the `relix` binary.
Most are pure Rust; a few bring native code:

| Component | Used by | Build requirement | Runtime requirement | License |
|---|---|---|---|---|
| DuckDB (bundled C++) | `relix-driver-duckdb` | C++ compiler; first build takes 10–15 minutes | none | MIT |
| ODPI-C (bundled C) | `relix-driver-oracle` | C compiler | **Oracle Instant Client** (64-bit Basic or Basic Light) | ODPI-C: UPL / Apache-2.0; Instant Client: Oracle's own license, not distributed with Relix |
| SQLite (bundled C) | `relix-driver-sqlite` | C compiler | none | public domain |
| aws-lc (C, via rustls in tiberius) | `relix-driver-mssql` | C compiler | none | Apache-2.0 / ISC |

TLS everywhere else uses rustls with the ring backend. Relix installs ring as the process-wide
rustls provider, so libraries that also compile aws-lc still use one crypto backend.

## Oracle Instant Client

1. Download the 64-bit *Basic* or *Basic Light* package for your platform from Oracle.
2. Unpack it, e.g. to `/opt/oracle/instantclient_23_26` or `C:\oracle\instantclient_23_26`.
3. Make it findable:
   * Linux: add the directory to `LD_LIBRARY_PATH` (or `ldconfig`); install `libaio`.
   * Windows: add it to `PATH`.
   * macOS: set the data source property `client_lib_dir` to the directory.

Without it, Oracle connections fail with *Oracle Instant Client not found* and these hints.

## External connectors

Engines that are not compiled in can be installed as signed external connectors from the
configured registries (`Space d`); see [external connectors](../drivers/external-connectors.md).
Relix never runs a downloaded binary before its signature is verified and you approved it.

## Testing

Unit and protocol tests run with `cargo test --workspace`. Integration tests against real
servers use `docker-compose.test.yml`:

```sh
docker compose -f docker-compose.test.yml up -d --wait postgres cockroachdb
RELIX_INTEGRATION=1 cargo test -p relix-driver-postgres -- --test-threads=1
```

Start only the services of the crates you test; CI runs one job per engine group. On
Apple Silicon the SQL Server service uses the Azure SQL Edge image; set
`MSSQL_IMAGE=mcr.microsoft.com/mssql/server:2022-latest` on x86-64. Snowflake and real BigQuery
cannot run locally; their protocol tests run without services.
