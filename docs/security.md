# Security model

Relix holds credentials for production databases and private networks, so it follows a few
strict rules.

## Secrets

* Passwords, SSH passwords and key passphrases are stored only in the operating system's
  credential store: macOS Keychain, Windows Credential Manager, or the Secret Service on
  Linux/BSD (service `relix`). Keys:
  * data sources: `project/<project>/datasource/<id>`
  * SSH configurations: `global/server/<id>/{password,passphrase}` or
    `project/<project>/server/<id>/{password,passphrase}`
* Configurations can be saved without secrets (*Save password* / *Save secret* off); Relix
  then asks when connecting and keeps the answer in memory for the session only.
* When no credential store is available Relix falls back to session memory and says so; it
  never writes secrets to disk itself. There is no home-grown encryption.
* Project, server and session files never contain secrets. They are written atomically with
  owner-only permissions (`0600`, directories `0700`) on Unix.
* Exported projects disable saved passwords and secrets, and duplicating or copying a server
  configuration never copies its stored secret.
* `Debug` output of secrets is redacted, and errors and logs are built without credentials.

## Network trust

* **SSH host keys are always verified.** Unknown keys need explicit approval after you see the
  fingerprint (or are refused with the strict policy); changed keys are always refused.
* **TLS** follows the data source's mode: `disable`, `prefer` (try TLS, fall back), `require`
  (encrypt, do not verify), `verify-full` (verify chain and host name against the Mozilla
  roots plus an optional CA file). Through SSH tunnels certificates are still verified against
  the original host name; where a client library cannot do that, `verify-full` is refused
  instead of being silently weakened.
* Cloud drivers use the providers' standard credential chains and keep short-lived access
  tokens in memory only.

## Privileged operations

Relix never runs as an administrator. Starting or stopping a managed VPN runs one command
through `pkexec`, the macOS administrator prompt or UAC after you approved the exact command
in Relix; arguments are never interpolated into an unquoted shell string. Relix only stops
VPN tunnels it started and that no data source uses any more.

## Drivers

Compiled-in drivers need no downloads. External connectors are only installed after you
approve them; packages are verified (size, SHA-256 and an ed25519 signature) before they are
unpacked, and they run as separate processes. See
[external connectors](drivers/external-connectors.md).
