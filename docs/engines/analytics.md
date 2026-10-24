# Analytics and cloud warehouses

Grid editing is read-only for all engines on this page: none of them can guarantee a correct,
transactional single-row update.

## ClickHouse

* HTTP interface: port 8123 (HTTPS usually 8443). Fields: host, port, user (empty means
  `default`), password, database, TLS mode.
* Properties: `auth_method` = `headers` (default, `X-ClickHouse-User/Key`) or `basic`;
  `session_timeout` (default 3600 s); `path_prefix` for reverse proxies.
* The console keeps a ClickHouse session, so `SET`, `USE` and temporary tables persist while
  the session is alive. Results stream; cancellation runs `KILL QUERY` for the statement's
  query id.
* Explorer: databases, tables, views, materialized views, dictionaries, columns with
  primary/sorting key flags and data-skipping indexes; definitions via `SHOW CREATE`.
* Limitations: binary data in `String` columns is shown with replacement characters; arrays,
  maps and tuples are shown as ClickHouse JSON.

## Trino

* Client REST protocol, port 8080. Fields: host, port, user (falls back to the OS user),
  database = catalog. Properties: `schema`, `session_properties` (`a=1,b=2`), `path_prefix`.
* A password is only sent over HTTPS (Basic authentication); `prefer` is upgraded to `require`
  when a password is set.
* Catalogs are listed as databases. `USE`, session properties, prepared statements and
  transactions are tracked through the protocol's session headers. Cancelling or reaching the
  row limit abandons the query on the server.
* No primary keys or indexes are reported (most connectors have none).

## Snowflake

* SQL API v2 over HTTPS to `<account>.snowflakecomputing.com` (host optional; required when
  using an SSH tunnel, which is supported because all calls go to the account host).
* Fields: user, database (default database), password slot = key passphrase or token.
* Properties: `account` (e.g. `myorg-myaccount`), `authenticator`, `private_key_file`,
  `warehouse`, `role`, `schema`.
* Authentication: `snowflake_jwt` (default, key pair: PKCS#8 key file, optionally encrypted),
  `oauth` or `programmatic_access_token` (token in the password field). **Password and browser
  login are not available through the SQL API** and are refused with an explanation.
* Statements run asynchronously; result partitions are fetched only up to the row limit;
  cancel uses the API's cancel endpoint. `USE ROLE/WAREHOUSE/DATABASE/SCHEMA` is remembered
  for later statements. Multi-statement transactions (`BEGIN … COMMIT` across statements) are
  refused because every API call is its own session.
* **Not yet verified against a real account** — only against a mock of the documented API.
  Value encodings of `DATE`/`TIME`/`TIMESTAMP_*` in particular need confirmation.

## Google BigQuery

* REST API v2; cloud target (SSH tunnels do not apply — use a VPN or the system proxy).
* Properties: `project_id`, `credentials_file` (service account JSON), `auth` = `adc`
  (default) / `service_account` / `none` (only with `endpoint`), `location`, `dataset`,
  `additional_projects`, `maximum_bytes_billed`, `endpoint` (e.g. the local emulator).
* Credentials follow Google's standard order: `GOOGLE_APPLICATION_CREDENTIALS`, gcloud's
  application default credentials, the metadata server, `gcloud auth print-access-token`.
  Tokens are kept in memory only.
* Projects are listed as databases, datasets as schemas. Unfiltered, unsorted table pages
  use the free `tabledata.list`; filtered or sorted pages run billed queries (cap them with
  `maximum_bytes_billed`). Console statements share one BigQuery session.
* Limitations: authorized-user and workload-identity credential files given through
  `GOOGLE_APPLICATION_CREDENTIALS` are not supported (gcloud user login works); verified
  against the open-source emulator and protocol tests, not yet against Google Cloud.
