# Reusable server configurations

SSH servers and VPN profiles are defined once, given a name, and referenced by any number
of data sources. Changing a configuration changes it for every data source that uses it —
no credentials or host names are copied into data sources.

Open the manager with **Space n** (or *Server configurations (SSH / VPN)* in the command
palette, `Ctrl+P`).

```text
╭ Server configurations ────────────────────────────────────────────────────────────────╮
│ SSH                                                                                    │
│ ├── Homelab SSH          global   admin@192.168.178.10:22 · private key   4 data sources │
│ └── Production Bastion   project  deploy@bastion.example.com:22 · SSH agent  connected │
│                                                                                        │
│ VPN                                                                                    │
│ └── Office WireGuard     global   WireGuard · managed by Relix   connected (managed)   │
│ n new SSH · v new VPN · Enter edit · d delete · y duplicate · r rename · t test · …     │
╰────────────────────────────────────────────────────────────────────────────────────────╯
```

| Key | Action |
|---|---|
| `n` / `v` | Create an SSH configuration / a VPN profile |
| `Enter`, `e` | Edit the selected configuration |
| `d`, `Del` | Delete (shows how many data sources and jump-host chains depend on it) |
| `y` | Duplicate (stored secrets are not copied) |
| `r` | Rename — references use stable ids, so every data source keeps working |
| `t` | Test: SSH opens a session through the whole jump chain; VPN shows diagnostics |
| `m` / `c` | Move / copy the configuration to the other scope (global ↔ project) |
| `Space` | Connect or disconnect a managed VPN profile |
| `Esc` | Close |

Each row shows the scope, the endpoint, how many data sources of the current project use it
and the live state (active SSH session, VPN status).

## Global and project scope

* **Global** configurations live in `<config>/servers.toml` and are available in every
  project — a personal VPS, the company VPN, a homelab bastion.
* **Project** configurations live in `<data>/projects/<id>/servers.toml` and are only visible
  inside that project — customer bastions, staging VPNs.

The scope is chosen when a configuration is created. *Move* keeps the configuration's
identity, so data sources keep resolving it; moving to global makes it (and its stored
secret) available to all projects, moving to project hides it from other projects. *Copy*
creates an independent configuration with a new identity and **never** copies stored
secrets, so credentials are not shared between projects by accident.

A global SSH configuration cannot use a project-specific jump host; such a chain is rejected
because it would break in every other project.

## Using a configuration from a data source

The data source dialog has a **Network** field:

| Mode | Meaning |
|---|---|
| `direct` | Connect to host:port directly (default) |
| `SSH tunnel` | Forward through the selected SSH configuration |
| `VPN` | Require the selected VPN profile, then connect directly |
| `VPN + SSH tunnel` | Bring up / verify the VPN, then tunnel through SSH |

With an SSH tunnel you can set a fixed **Local port**; otherwise a free port is picked
automatically. The host and port of the data source are always the database as seen from
the SSH server (for example `10.0.0.15:5432`).

The explorer shows how each source is reached (`connected via SSH Production Bastion`,
`connected via VPN Office WireGuard`, `connected directly`); a degraded tunnel (lost,
reconnecting) is shown in amber.

## Files

```toml
# <config>/servers.toml  (or <data>/projects/<id>/servers.toml)
format_version = 1

[[ssh]]
id = "4f0d…"
name = "Production Bastion"
host = "bastion.example.com"
port = 22
username = "deploy"
auth = "private_key"          # password, private_key, agent
private_key = "~/.ssh/deploy_ed25519"
save_secret = true            # passphrase / password in the OS credential store
use_ssh_config = false        # read HostName/Port/User/IdentityFile from ~/.ssh/config
host_key_policy = "ask_on_first_use"   # or "strict"
connect_timeout_secs = 15
keepalive_interval_secs = 30  # 0 disables keepalives
jump_host = "9a2c…"           # id of another SSH configuration (optional)
local_bind_address = "127.0.0.1"

[ssh.reconnect]
enabled = true
max_attempts = 3
delay_secs = 2

[[vpn]]
id = "c31e…"
name = "Office WireGuard"
mode = "managed"              # existing, managed
provider = "wire_guard"       # wire_guard, open_vpn, other
config_path = "~/.config/relix/vpn/office.conf"
interface = "office"          # optional, used to verify the tunnel
probe_address = "10.10.0.1:22" # optional host:port reachable only through the VPN
auto_connect = true
auto_disconnect = true
connect_timeout_secs = 30
```

Data sources reference configurations by id:

```toml
[data_sources.network]
mode = "vpn_ssh_tunnel"       # direct, ssh_tunnel, vpn, vpn_ssh_tunnel
ssh = "4f0d…"
vpn = "c31e…"
local_port = 15432            # optional
```

Files never contain passwords or passphrases. A file that cannot be parsed is moved aside
(`servers.corrupt-N`) and reported, never silently discarded; a file written by a newer
Relix version is opened read-only. A data source whose configuration was deleted reports
*"the referenced SSH configuration no longer exists"* when it connects.

Exporting a project includes its project-scoped configurations (secrets excluded, *save
secret* turned off); importing gives them new ids and rewrites every reference.
