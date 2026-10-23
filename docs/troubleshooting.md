# Troubleshooting connections

Every connection failure names the stage that failed — **configuration**, **VPN**,
**SSH tunnel** or **database** — so start there. Logs are written to the directory shown by
`relix --print-paths` (set `RELIX_LOG=debug` for more detail; logs never contain secrets).

## Configuration

| Message | Meaning |
|---|---|
| `the referenced SSH configuration no longer exists` | The data source points at a deleted configuration; pick another one in the data source dialog |
| `jump hosts form a loop: A → B → A` | Fix the *Jump host* fields |
| `global configuration 'X' cannot use a project-specific jump host` | Move the jump host to global scope, or the configuration to the project |
| `… talks to cloud APIs; SSH tunnels cannot carry it` | Use `direct` or `VPN` for BigQuery / DynamoDB |

## VPN

* `interface 'wg0' of 'Office' is not up` — the existing VPN is not connected, or the
  interface name differs (`ip link` / `ifconfig` / `Get-NetAdapter` list them).
* `… is not reachable through 'Office'` — the check address does not answer; verify routes
  and firewall. Press `t` on the profile for diagnostics (interface, probe, announced routes).
* `'wg-quick' was not found` — install wireguard-tools (Homebrew on macOS) or the WireGuard
  app on Windows.
* `administrator rights were not granted` — the system prompt was cancelled or polkit denied
  it (Linux: a polkit agent must be running for `pkexec`).
* `routes … already used by the active VPN` — two managed profiles overlap; disconnect the
  other one first.
* `'X' did not come up within 30s` — check the VPN's own logs (OpenVPN writes
  `<data>/vpn/<id>.log`).

## SSH

* `HOST KEY MISMATCH for host:port` — the server presented a different key than recorded.
  Do not continue until its administrator confirmed a key change; then delete the line named
  in the message from the known hosts file.
* `the … host key of host:port is not known` with the strict policy — add the verified key to
  `<config>/ssh/known_hosts` or `~/.ssh/known_hosts`.
* `SSH authentication as 'user' on host:port failed` — wrong user, password or key (the
  server must accept the key in `authorized_keys`).
* `key passphrase for SSH server 'X' is required` — the key is encrypted; enter the passphrase.
* `SSH agent unavailable: SSH_AUTH_SOCK is not set` — start `ssh-agent` (or Pageant / the
  Windows OpenSSH agent service) and add your key.
* `jump host refused the connection` — the gateway cannot reach the next hop (DNS, firewall,
  `AllowTcpForwarding no` on the gateway).
* `SSH tunnel to host:port failed` — the SSH server cannot reach the database (wrong host or
  port from the server's point of view, or forwarding disabled).
* `local address 127.0.0.1:15432 is unavailable` — the fixed local port is in use; clear the
  *Local port* field to choose one automatically.

## Database

* `authentication failed` — Relix asks for the password again (the database password, not the
  SSH one).
* `TLS handshake failed` / certificate errors with `verify-full` — set the CA certificate, or
  check that the certificate matches the host name you configured (through a tunnel the
  original name is used).
* `connection lost` — the database or the tunnel dropped; the next action reconnects.
  Statements that may have run are never repeated automatically.
