# Configuration

## Locations

`relix --print-paths` prints the directories RELIX uses. They follow the platform
conventions (`~/.config/relix` and `~/.local/share/relix` on Linux, `~/Library/Application
Support/dev.t0bx.relix` on macOS, `%APPDATA%\t0bx\relix` on Windows). Setting the
environment variable `RELIX_HOME` puts everything below one directory instead
(`$RELIX_HOME/config`, `$RELIX_HOME/data`, `$RELIX_HOME/cache`), which is handy for portable
installations and tests.

| File | Purpose |
|---|---|
| `config/config.toml` | global settings |
| `config/themes/*.toml` | custom themes |
| `data/recent.toml` | recently opened projects |
| `data/projects/<id>/project.toml` | project metadata and data sources |
| `data/projects/<id>/session.json` | workspace layout, tabs, connected sources, explorer state |
| `data/projects/<id>/drafts/`, `scripts/` | unsaved editor contents and saved scripts |
| `data/projects/<id>/history.jsonl`, `filters.json` | query history and WHERE filter history |
| `data/drivers/` | installed external driver packages |
| `data/logs/relix.log.*` | logs (level via `RELIX_LOG`, e.g. `RELIX_LOG=debug`) |

Files are written atomically and readable by the owner only. If `config.toml` cannot be
parsed it is renamed to `config.corrupt-N` and defaults are used; the reason is shown as a
notification. Values outside their valid range are clamped and reported.

## `config.toml`

```toml
config_version = 1
theme = "dark"                  # dark, light, high-contrast or a custom theme name

[ui]
show_hints = true               # contextual key hint line above the status bar
sidebar_width = 34              # 16 – 120 columns
mouse = true                    # click to focus, wheel to scroll (disable with --no-mouse)
show_line_numbers = true
unicode_icons = true            # use ASCII icons when your font lacks the glyphs

[editor]
tab_width = 4
auto_complete = true            # open completion while typing identifiers or after "."

[query]
row_limit = 500                 # rows fetched per editor statement (has_more is shown)
page_size = 200                 # rows per page in table tabs and re-queried results
timeout_secs = 300              # cancel statements after this many seconds, 0 = never
confirm_destructive = true      # confirm DROP/TRUNCATE/unbounded DELETE+UPDATE on staging/production
history_limit = 1000            # query history entries kept per project

[drivers]
offline = false                 # never contact package registries

[[drivers.registries]]
name = "Example registry"
url = "https://drivers.example.com/index.json"
public_key = "base64-encoded ed25519 public key"

[keybindings]
"palette.open" = ["ctrl+p", "f2"]   # replaces the default keys of a command
"grid.json" = "ctrl+j"
"tab.close" = []                    # an empty list unbinds the command

[theme_overrides]
accent = "#ff8800"
selection_bg = "236"
```

### Key binding syntax

A binding is a sequence of chords separated by spaces (`"ctrl+k s"`). A chord is a key with
optional `ctrl+`, `alt+` and `shift+` prefixes. Keys are single characters (`a`, `G`, `?`,
`/`), `enter`, `esc`, `tab`, `backtab`, `backspace`, `delete`, `insert`, `up`, `down`,
`left`, `right`, `home`, `end`, `pageup`, `pagedown`, `space` and `f1` … `f24`. Upper-case
letters imply shift. All command ids are listed in [keybindings.md](keybindings.md).

Note that many terminals cannot distinguish `Ctrl+Enter` from `Enter` or `Ctrl+H` from
`Backspace`. RELIX enables the kitty keyboard protocol where supported; otherwise use the
alternatives (`F5`, `Alt+Enter`, …).

## Themes

Every colour of the UI is a theme key:

```
background surface foreground muted border border_focused accent accent_secondary
selection_bg selection_fg cursor_line status_bg status_fg error warning success info
syntax_keyword syntax_string syntax_number syntax_comment syntax_identifier syntax_operator
syntax_function syntax_type grid_header_bg grid_header_fg grid_row_alt_bg grid_null
grid_modified_bg grid_inserted_bg grid_deleted_fg grid_cursor_bg
env_production env_staging env_testing env_development
```

Colours are `#rrggbb`, a 256-colour index (`"236"`), a named ANSI colour (`red`,
`lightblue`, `darkgray`, …) or `reset`. Truecolour is used when the terminal sets
`COLORTERM=truecolor`; otherwise RGB values are mapped to the closest xterm-256 colour.

A custom theme lives in `config/themes/<anything>.toml` and extends a built-in theme:

```toml
name = "midnight"
extends = "dark"          # dark, light or high-contrast
borders = "plain"         # plain, rounded, double or thick

[colors]
accent = "#e0af68"
border_focused = "#e0af68"
grid_modified_bg = "#3b2f1e"
```

Select it with `theme = "midnight"`, `relix --theme midnight` or from the command palette.
Unknown colour names and invalid files are reported as warnings.

## Projects and data sources

`project.toml` contains no secrets and can be committed or shared; `Export project` in the
palette writes a portable copy and marks passwords as "not saved" so recipients are
prompted.

```toml
format_version = 1
id = "1c0f…"
name = "Development"
created_at = "2026-10-23T08:00:00Z"

[settings]
row_limit = 1000            # overrides [query].row_limit for this project
confirm_destructive = true  # overrides [query].confirm_destructive

[[data_sources]]
id = "7a4e…"
name = "Main PG"
driver = "postgres"         # postgres, mysql, mariadb, sqlite, redis or an external driver id
environment = "production"  # none, development, testing, staging, production
color = "#ff5370"

[data_sources.settings]
host = "db.internal"
port = 5432
username = "app"
database = "app"
connect_timeout_secs = 10
save_password = true        # store the password in the OS credential store

[data_sources.settings.tls]
mode = "verify_full"        # disable, prefer, require, verify_full
ca_certificate = "/etc/ssl/internal-ca.pem"
```

SQLite data sources use `file_path` (or `":memory:"`) and `create_if_missing`; Redis uses
`database` for the numeric database index.

### Passwords

Passwords are stored in the operating system's credential store under the service
`relix` (macOS Keychain, Windows Credential Manager, the Secret Service on Linux/BSD).
When no credential store is available, or when *Save password* is off, RELIX asks for the
password when connecting and keeps it in memory for the session only. Passwords never
appear in project files, session files or logs.
