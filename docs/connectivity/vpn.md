# VPN connections

VPN support is optional and independent of SSH tunnels. A data source uses a VPN profile
when its **Network** mode is `VPN` or `VPN + SSH tunnel`.

## Existing system VPN

Select **Connection: existing system VPN** when the operating system (or your VPN client)
manages the VPN. Relix never starts, stops or modifies it; it only verifies it before
connecting:

* **Interface** (e.g. `wg0`, `utun4`, `tun0`): the interface must exist.
* **Check address** (`host:port`): must accept a TCP connection — pick something only
  reachable through the VPN.

Without either, Relix cannot tell whether the VPN is up and simply connects. Press `t` in
the server manager (or `Ctrl+T` in the dialog) for diagnostics: interface state and
addresses, probe result and the routes the configuration file announces.

## Profiles managed by Relix

Select **Connection: managed by Relix** to let Relix bring a WireGuard or OpenVPN tunnel up
and down using the tools installed on your system — Relix contains no VPN implementation.

| Platform | WireGuard | OpenVPN |
|---|---|---|
| Linux | `wg-quick up/down <file>` (wireguard-tools) | `openvpn --config <file> --daemon` (stopped by its PID) |
| macOS | `wg-quick` from Homebrew `wireguard-tools` | `openvpn` from Homebrew |
| Windows | `wireguard.exe /installtunnelservice` / `/uninstalltunnelservice` | not managed — connect with OpenVPN GUI and use *existing system VPN* |

WireGuard files must be named `<name>.conf` with at most 15 characters (letters, digits,
`_ = + . -`), as `wg-quick` requires. The interface name is derived from the file name (on
macOS from `wg-quick`'s `utunN` mapping) unless you set one.

### Privileges and consent

Bringing a VPN up needs administrator rights. Relix never runs as root. Instead it runs
**exactly one** command elevated through the operating system:

* Linux: `pkexec` (polkit asks for your password),
* macOS: `osascript … with administrator privileges` (the standard macOS prompt),
* Windows: `Start-Process -Verb RunAs` (UAC).

Before the first connection of a session Relix shows the full command and only runs it
after you approve; the operating system then asks for confirmation as well. Arguments are
passed without a shell (Linux) or quoted for the respective interpreter (macOS, Windows);
paths with control characters are refused. Refusing the system prompt is reported as
*administrator rights were not granted*.

### Lifecycle

1. If the profile's interface already exists, the VPN is treated as system-managed and never
   touched.
2. Otherwise, with *Auto connect* on (or after **Connect** in the dialog / `Space` in the
   manager) Relix starts it, waits until the interface (or check address) is available and
   records that it owns the tunnel.
3. Every data source using the profile holds a reference. With *Auto disconnect* on, Relix
   stops the tunnel when the last data source using it disconnects. A tunnel connected
   manually stays up until you disconnect it.
4. **Disconnect** refuses while data sources still use the tunnel and never disconnects a VPN
   Relix did not start.

### Conflicting routes

Before starting a managed profile Relix compares the networks it routes (WireGuard
`AllowedIPs`, OpenVPN `route` directives) with every other tunnel it manages. Overlapping
routes are reported (*routes 10.10.5.0/24 already used by the active VPN 'Office'*) instead of
starting a tunnel that would break an existing one. Profiles that route all traffic
(`0.0.0.0/0`, `redirect-gateway`) are flagged in diagnostics.

## VPN + SSH

With `VPN + SSH tunnel` the VPN is established (or verified) first, then the SSH session to
the bastion inside the private network, then the database connection through the tunnel.

```text
Relix ─▶ Corporate WireGuard ─▶ SSH bastion ─▶ internal PostgreSQL
```

## Limitations

* OpenVPN profiles that use interactive authentication (username/password prompts,
  challenge/response) cannot be started unattended; use *existing system VPN* for them.
* OpenVPN without an interface or check address is considered up as soon as its PID file
  exists.
* Managed OpenVPN is not available on Windows.
