# Keyboard shortcuts

All shortcuts are defined in one command registry (`crates/relix-app/src/command.rs`) and can be
changed in `config.toml` under `[keybindings]` using the command ids below. Inside RELIX, `F1`
shows the bindings that are active for the focused region.

Key contexts are checked from the most specific to the most general one: for example the data
grid uses *Data grid → Lists → Outside text inputs → Global*. In text inputs (SQL editor, filter
bar, prompts, console) printable keys are typed instead of being treated as shortcuts.

The leader key is `Ctrl+K` everywhere and `Space` outside text inputs; after pressing it a
which-key popup lists the possible continuations.

## Global — available everywhere

| Keys | Action | Command id |
|---|---|---|
| `Ctrl+Q`, `Ctrl+K q` / `Space q` | Quit RELIX | `app.quit` |
| `Ctrl+K w` / `Space w` | Save script as… | `editor.save_as` |
| `Ctrl+K o` / `Space o` | Open SQL file… | `editor.open_file` |
| `Ctrl+K a` / `Space a` | Add data source | `datasource.add` |
| `Ctrl+K n` / `Space n` | Server configurations (SSH / VPN) | `servers.open` |
| `Ctrl+K d` / `Space d` | Driver manager | `drivers.show` |
| `Ctrl+K h` / `Space h` | Query history | `history.show` |
| `Ctrl+K t` / `Space t` | Cycle color theme | `theme.cycle` |
| `Ctrl+K c` / `Space c` | Connect / disconnect | `datasource.toggle_leader` |
| palette only | Export project (without credentials) | `project.export` |
| palette only | Import project | `project.import` |
| `Ctrl+P` | Command palette | `palette.open` |
| `Ctrl+O`, `Ctrl+K p` / `Space p` | Open / manage projects | `project.open` |
| `Ctrl+N` | New query tab | `editor.new` |
| `Ctrl+S` | Save script / commit pending changes | `app.save` |
| `Ctrl+W` | Close tab | `tab.close` |
| `F6` | Focus next region | `focus.next` |
| `Shift+F6` | Focus previous region | `focus.previous` |
| `Ctrl+E`, `Ctrl+K e` / `Space e` | Focus database explorer | `focus.explorer` |
| `Ctrl+B`, `Ctrl+K b` / `Space b` | Toggle sidebar | `view.sidebar` |
| `F1` | Contextual help | `help.show` |
| `Ctrl+G`, `Ctrl+K j` / `Space j` | Jump to pane / tab / region | `jump.regions` |
| `Alt+V`, `Ctrl+K v` / `Space v` | Split pane vertically | `pane.split_vertical` |
| `Alt+S`, `Ctrl+K s` / `Space s` | Split pane horizontally | `pane.split_horizontal` |
| `Alt+X`, `Ctrl+K x` / `Space x` | Close pane | `pane.close` |
| `Alt+H`, `Alt+←` | Focus pane left | `pane.focus_left` |
| `Alt+L`, `Alt+→` | Focus pane right | `pane.focus_right` |
| `Alt+K`, `Alt+↑` | Focus pane above | `pane.focus_up` |
| `Alt+J`, `Alt+↓` | Focus pane below | `pane.focus_down` |
| `Alt+=`, `Alt++` | Grow pane | `pane.grow` |
| `Alt+-` | Shrink pane | `pane.shrink` |
| `Alt+.`, `Ctrl+PgDn` | Next tab | `tab.next` |
| `Alt+,`, `Ctrl+PgUp` | Previous tab | `tab.previous` |
| `Alt+1` | Tab 1 | `tab.select_1` |
| `Alt+2` | Tab 2 | `tab.select_2` |
| `Alt+3` | Tab 3 | `tab.select_3` |
| `Alt+4` | Tab 4 | `tab.select_4` |
| `Alt+5` | Tab 5 | `tab.select_5` |
| `Alt+6` | Tab 6 | `tab.select_6` |
| `Alt+7` | Tab 7 | `tab.select_7` |
| `Alt+8` | Tab 8 | `tab.select_8` |
| `Alt+9` | Tab 9 | `tab.select_9` |
| `Ctrl+F` | Search / filter | `view.filter` |
| `Ctrl+Enter`, `F5`, `Alt+Enter` | Execute statement or selection | `query.run` |
| `Ctrl+F2`, `F8` | Cancel running query | `query.cancel` |

## Outside text inputs

| Keys | Action | Command id |
|---|---|---|
| `Tab` | Focus next region | `focus.next_tab_key` |
| `Shift+Tab` | Focus previous region | `focus.previous_tab_key` |
| `?` | Contextual help | `help.show_question` |
| `r`, `F5` | Refresh | `view.refresh` |
| `/` | Search / filter | `view.filter_slash` |

## Lists, explorer and grids

| Keys | Action | Command id |
|---|---|---|
| `↑`, `k` | Up | `nav.up` |
| `↓`, `j` | Down | `nav.down` |
| `←`, `h` | Left | `nav.left` |
| `→`, `l` | Right | `nav.right` |
| `PgUp`, `Ctrl+U` | Page up | `nav.page_up` |
| `PgDn`, `Ctrl+D` | Page down | `nav.page_down` |
| `g`, `Ctrl+Home` | Top | `nav.top` |
| `G`, `Ctrl+End` | Bottom | `nav.bottom` |
| `Home`, `0` | First column | `nav.line_start` |
| `End`, `$` | Last column | `nav.line_end` |

## Database explorer

| Keys | Action | Command id |
|---|---|---|
| `Enter`, `o` | Open / expand | `explorer.activate` |
| `Backspace` | Collapse | `explorer.collapse` |
| `n`, `q` | New query for data source | `explorer.new_query` |
| `O` | Open table in new pane | `explorer.open_split` |
| `e` | Edit data source | `datasource.edit` |
| `y` | Duplicate data source | `datasource.duplicate` |
| `D`, `Del` | Remove data source | `datasource.delete` |
| `c` | Connect / disconnect | `datasource.toggle` |
| `i` | Inspect object / DDL | `explorer.inspect` |

## SQL editor

| Keys | Action | Command id |
|---|---|---|
| `Ctrl+Shift+Enter`, `F9` | Execute entire script | `query.run_script` |
| `Ctrl+Space` | Trigger completion | `editor.complete` |
| `Ctrl+F` | Find | `editor.find` |
| `Ctrl+R` | Find and replace | `editor.replace` |
| `F3` | Find next | `editor.find_next` |
| `Shift+F3` | Find previous | `editor.find_previous` |
| `Ctrl+Z` | Undo | `editor.undo` |
| `Ctrl+Y`, `Ctrl+Shift+Z` | Redo | `editor.redo` |
| `Ctrl+C` | Copy | `editor.copy` |
| `Ctrl+X` | Cut | `editor.cut` |
| `Ctrl+V` | Paste | `editor.paste` |
| `Ctrl+A` | Select all | `editor.select_all` |
| `Ctrl+/`, `Ctrl+_`, `Ctrl+7` | Toggle line comment | `editor.comment` |
| `Ctrl+D` | Duplicate line | `editor.duplicate_line` |
| `Ctrl+L` | Delete line | `editor.delete_line` |
| `Ctrl+T` | Choose data source for query tab | `editor.datasource` |
| `Ctrl+↓` | Focus results | `editor.focus_results` |

## Data grid (tables and query results)

| Keys | Action | Command id |
|---|---|---|
| `Ctrl+↑` | Focus editor | `results.focus_editor` |
| `x` | Select / unselect row | `grid.toggle_row` |
| `Shift+↑`, `K` | Extend selection up | `grid.extend_up` |
| `Shift+↓`, `J` | Extend selection down | `grid.extend_down` |
| `Ctrl+A` | Select all rows | `grid.select_all` |
| `Enter`, `e`, `F2` | Edit cell | `grid.edit` |
| `Alt+E` | Edit cell in multi-line editor | `grid.edit_popup` |
| `N` | Set cell to NULL | `grid.set_null` |
| `Ins`, `a` | Insert row | `grid.insert` |
| `Del`, `d` | Delete selected rows | `grid.delete` |
| `u` | Revert pending change | `grid.revert` |
| `Ctrl+S` | Review & commit pending changes | `grid.review` |
| `U` | Discard all pending changes | `grid.discard` |
| `s` | Sort by column (cycle) | `grid.sort` |
| `S` | Add column to multi-sort | `grid.sort_add` |
| `o` | Edit sort rules | `grid.sort_edit` |
| `w`, `/` | Edit WHERE filter | `grid.filter` |
| `v` | Toggle grid / JSON view | `grid.json` |
| `>` | Widen column | `grid.widen` |
| `<` | Narrow column | `grid.narrow` |
| `-` | Hide column | `grid.hide_column` |
| `c` | Show / hide columns | `grid.columns` |
| `y`, `Ctrl+C` | Copy cell value | `grid.copy_cell` |
| `Y` | Copy selected rows (TSV) | `grid.copy_rows` |
| `i` | Inspect cell value | `grid.inspect` |
| `]`, `n` | Next page | `grid.next_page` |
| `[`, `p` | Previous page | `grid.previous_page` |
| `#` | Count rows | `grid.count` |
| `E` | Export results (CSV / JSON) | `grid.export` |
| `f` | Jump to row | `grid.jump_row` |
| `F` | Jump to column | `grid.jump_column` |

## Redis key browser

| Keys | Action | Command id |
|---|---|---|
| `Enter` | Open key | `kv.open` |
| `m` | Load more keys / values | `kv.load_more` |
| `a`, `Ins` | Create key | `kv.create` |
| `d`, `Del` | Delete key / element | `kv.delete` |
| `e` | Edit value / element | `kv.edit` |
| `t` | Set TTL | `kv.ttl` |
| `:` | Focus command console | `kv.console` |
| `y` | Copy value | `kv.copy` |

## Document browser (MongoDB, DynamoDB, Elasticsearch, OpenSearch)

| Keys | Action | Command id |
|---|---|---|
| `Enter`, `e` | Edit document (JSON editor) | `doc.open`, `doc.edit` |
| `a`, `Ins` | Insert document | `doc.insert` |
| `d`, `Del` | Delete document | `doc.delete` |
| `/` | Edit the filter (native JSON filter, `Enter` applies) | `doc.filter` |
| `]`, `n` | Next page | `doc.next_page` |
| `[`, `p` | Previous page | `doc.previous_page` |
| `i` | Collection details and indexes | `doc.details` |
| `+` | Create index | `doc.create_index` |
| `-` | Drop index | `doc.drop_index` |
| `y` | Copy document JSON | `doc.copy` |

## Inside popups

| Popup | Keys |
|---|---|
| Pickers (palette, projects, history, drivers) | type to filter, `↑`/`↓`, `Enter` accept, `Esc` close; projects: `Ctrl+N` new, `Ctrl+R` rename, `Ctrl+D` delete; history: `Ctrl+R` run again; drivers: `Ctrl+U` check registries, `Ctrl+D` remove |
| Data source, SSH and VPN dialogs | `Tab`/`↑`/`↓` move, `←`/`→` change choice, `Space` toggle, `Ctrl+T` test, `Enter`/`Ctrl+S` save, `Esc` cancel; VPN dialog: `Ctrl+O` connect, `Ctrl+X` disconnect |
| Server configurations | `n` new SSH, `v` new VPN, `Enter`/`e` edit, `d` delete, `y` duplicate, `r` rename, `t` test, `m` move / `c` copy to the other scope, `Space` connect/disconnect VPN, `Esc` close |
| SSH host key / VPN approval | read the fingerprint or command, `Enter`/`y` to trust / run, `Esc` to refuse |
| Confirmation | `Enter`/`y` confirm, `Esc`/`n` cancel |
| Viewer (DDL, cell inspector) | `↑`/`↓`/`PgUp`/`PgDn` scroll, `y` copy, `Esc` close |
| Multi-line editor | normal editing keys, `Ctrl+S` save, `Ctrl+Z` undo, `Esc` cancel |
| Sort rules | `↑`/`↓` select, `Space` flip direction, `d` remove, `J`/`K` move, `Esc` close |
| Columns | `Space` show/hide, `a` show all, `Esc` close |
| Flash jump | type the label shown next to the target |
| Editor search | type to search, `Enter`/`↓` next, `↑` previous, `Tab` switch to replace field, `Enter` in replace field replaces, `Ctrl+A` replace all, `Alt+C` case sensitivity, `Esc` close |
| WHERE filter bar | `Enter` apply, `Esc` back to grid, `↑`/`↓` filter history, `Ctrl+Space` completion, `Tab` accept completion |
| Redis console | `Enter` run, `↑`/`↓` history, `Esc` back to keys |
