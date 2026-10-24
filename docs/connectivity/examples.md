# Connection examples

## 1. PostgreSQL through an SSH bastion

The database `10.0.0.15:5432` is only reachable from `bastion.example.com`.

1. `Space n` → `n` (new SSH configuration):
   * Name `Production Bastion`, scope `project`
   * SSH host `bastion.example.com`, port `22`, user `deploy`
   * Auth method `private key`, private key `~/.ssh/deploy_ed25519`
   * `Ctrl+T` tests the session. The first time Relix shows the host key fingerprint —
     compare it with the one your administrator published, then confirm.
   * `Enter` saves.
2. `Space a` (new data source):
   * Engine `PostgreSQL`, host `10.0.0.15`, port `5432`, database `production`, user
     `postgres`
   * Network `SSH tunnel`, SSH config `Production Bastion`
   * `Ctrl+T` tests the whole route, `Enter` saves.

The explorer shows `connected via SSH Production Bastion`. With TLS mode `verify-full` the
certificate is verified against `10.0.0.15`'s configured name, not against the local tunnel
endpoint.

## 2. Redis through WireGuard

The company WireGuard tunnel is managed by the WireGuard app.

1. `Space n` → `v`: Name `Office WireGuard`, Connection `existing system VPN`,
   Interface `utun4` (macOS) / `wg0` (Linux) / `Office` (Windows),
   Check address `10.10.0.20:6379`.
2. Data source: Engine `Redis`, host `10.10.0.20`, port `6379`, Network `VPN`,
   VPN profile `Office WireGuard`.

If the tunnel is down, connecting fails with *network interface 'utun4' of 'Office
WireGuard' is not up* instead of a generic timeout. To let Relix bring the tunnel up itself,
switch the profile to *managed by Relix*, select the `.conf` file and enable *Auto connect*;
Relix asks for approval before running `wg-quick up` with administrator rights.

## 3. One SSH configuration for five data sources

Create `Homelab SSH` once (global scope, `admin@192.168.178.10`, SSH key) and select it as
SSH config in the PostgreSQL, MariaDB, Redis, MongoDB and ClickHouse data sources of the
`Homelab` project. All five share one SSH session (the server manager shows it as
`connected · 5 connection(s)`). Closing one data source keeps the session for the others; it closes
with the last one. Changing the key path in `Homelab SSH` applies to all five on their next
connect.

## 4. WireGuard plus an SSH jump host

```text
Relix ─▶ Corporate WireGuard ─▶ gateway.corp (jump host) ─▶ db-bastion.corp ─▶ 10.20.0.5:5432
```

1. VPN profile `Corporate WireGuard` (managed, *Auto connect* and *Auto disconnect* on).
2. SSH configuration `Corp Gateway` (`gateway.corp`).
3. SSH configuration `DB Bastion` (`db-bastion.corp`) with *Jump host* `Corp Gateway`.
4. Data source with host `10.20.0.5`, Network `VPN + SSH tunnel`, VPN profile
   `Corporate WireGuard`, SSH config `DB Bastion`.

Relix brings up WireGuard (after your approval), connects to `gateway.corp`, opens the
second SSH session through it to `db-bastion.corp`, forwards a local port to `10.20.0.5:5432`
and connects PostgreSQL to it. Disconnecting the last data source closes both SSH sessions
and stops the WireGuard tunnel Relix started.

## 5. MongoDB with authentication and TLS

* Engine `MongoDB`, host `mongo.internal`, port `27017`, user `app`, password (saved in the
  keyring), database `shop`.
* TLS `verify-full`, CA cert `/etc/ssl/internal-ca.pem`.
* Properties `authSource=admin; replicaSet=rs0`.

For Atlas or other SRV deployments use the property
`uri=mongodb+srv://app@cluster0.example.mongodb.net/shop?retryWrites=true` and keep the
password in the password field — it fills in the credentials the URI leaves out, so the
secret stays in the keyring. Through an SSH tunnel use `require` instead of `verify-full`
(the MongoDB client verifies the dialled host), or reach the cluster via VPN.

## 6. Cloud databases with provider credentials

**BigQuery** — run `gcloud auth application-default login` once (or point
`GOOGLE_APPLICATION_CREDENTIALS` at a service account key), then create a data source with
engine `BigQuery` and the property `project_id=analytics-prod; location=EU`. No secret is
stored by Relix; access tokens stay in memory.

**DynamoDB** — configure the AWS CLI (`aws configure sso` or a profile in `~/.aws/config`),
then use engine `DynamoDB` with `region=eu-central-1; profile=prod`. Leave user and password
empty so the AWS credential chain is used.

**Snowflake** — create a key pair as described in Snowflake's key-pair authentication guide,
register the public key on your user, then set user `ANALYST`, password = the key's
passphrase, and the properties `account=myorg-myaccount; private_key_file=~/.snowflake/rsa_key.p8;
warehouse=COMPUTE_WH; role=ANALYST`.

Cloud APIs cannot be carried by an SSH port forward (BigQuery and DynamoDB use several
provider endpoints); use `direct` or `VPN`. Snowflake talks to a single account host and can
be tunnelled.
