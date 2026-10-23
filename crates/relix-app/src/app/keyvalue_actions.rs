/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use super::connections::Deferred;
use super::explorer_actions::edit_line;
use crate::command::Command;
use crate::events::AppEvent;
use crate::input::LineInput;
use crate::keyvalue_tab::{
    KEY_SCAN_BATCH, KeyValueFocus, KeyValueTab, STRING_PREVIEW_BYTES, VALUE_PAGE_SIZE, ValueContent,
};
use crate::loaders;
use crate::notifications::Level;
use crate::overlay::{
    ConfirmState, Overlay, PendingAction, PromptAction, PromptState, TextEditAction, TextEditState,
};
use crate::requests::RequestId;
use crate::tab::Tab;
use crate::workspace::TabId;
use relix_config::{KeyChord, KeyCode};
use relix_core::DataSourceId;
use relix_core::keyvalue::{
    CommandReply, KeyDescription, KeyName, KeyScanPage, KeyType, KeyValueWrite, ValuePage,
    ValueRange,
};
use relix_driver_api::DriverError;
use relix_editor::TextBuffer;

const KEY_PAGE: isize = 15;

impl App {
    pub(crate) fn open_key_value_tab(&mut self, source: DataSourceId, new_pane: bool) {
        let existing = self.workspace.tabs().find_map(|(id, tab)| match tab {
            Tab::KeyValue(tab) if tab.data_source == source => Some(id),
            _ => None,
        });
        if new_pane {
            self.workspace
                .split(relix_project::SplitDirection::Vertical);
        } else if let Some(existing) = existing {
            self.workspace.activate_tab(existing);
            self.focus_workspace();
            return;
        }
        let pane = self.workspace.focused_pane();
        let id = self
            .workspace
            .add_tab(pane, Tab::KeyValue(Box::new(KeyValueTab::new(source))));
        self.focus_workspace();
        self.start_key_scan(id, true);
    }

    fn kv_tab(&mut self, tab: TabId) -> Option<&mut KeyValueTab> {
        self.workspace.tab_mut(tab).and_then(Tab::as_key_value_mut)
    }

    /// Fetches the next SCAN batch (or restarts the scan). SCAN is incremental and
    /// non-blocking on the server, unlike KEYS.
    pub(crate) fn start_key_scan(&mut self, tab: TabId, restart: bool) {
        let request = self.next_request();
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        if restart {
            kv.reset_scan();
        } else if kv.scan_complete {
            return;
        }
        let source = kv.data_source;
        let cursor = kv.scan_cursor;
        let pattern = if kv.pattern.is_empty() {
            "*".to_owned()
        } else {
            kv.pattern.text().to_owned()
        };
        kv.scanning = Some(request);
        let Some(connection) = self.connection_or_defer(source, Deferred::ScanKeys(tab)) else {
            return;
        };
        if restart {
            self.load_server_details(tab, connection.clone());
        }
        self.spawn(async move {
            let result = match loaders::key_value(connection.as_ref()) {
                Ok(store) => store.scan_keys(cursor, &pattern, KEY_SCAN_BATCH).await,
                Err(error) => Err(error),
            };
            AppEvent::KeysScanned {
                tab,
                request,
                result,
            }
        });
    }

    fn load_server_details(
        &self,
        tab: TabId,
        connection: std::sync::Arc<dyn relix_driver_api::DatabaseConnection>,
    ) {
        self.spawn(async move {
            let result = match loaders::key_value(connection.as_ref()) {
                Ok(store) => store.server_details().await,
                Err(error) => Err(error),
            };
            AppEvent::ServerDetails { tab, result }
        });
    }

    pub(crate) fn on_keys_scanned(
        &mut self,
        tab: TabId,
        request: RequestId,
        result: Result<KeyScanPage, DriverError>,
    ) {
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        if kv.scanning != Some(request) {
            return;
        }
        kv.scanning = None;
        match result {
            Ok(page) => {
                let first_batch = kv.keys.is_empty();
                kv.scan_cursor = page.next_cursor;
                kv.scan_complete = page.is_complete();
                kv.keys.extend(page.keys);
                kv.keys.sort();
                kv.keys.dedup();
                let count = kv.keys.len();
                let complete = kv.scan_complete;
                let want_more = !complete && count < KEY_SCAN_BATCH as usize;
                self.status = format!(
                    "{count} keys{}",
                    if complete {
                        ""
                    } else {
                        " (scan in progress, m: more)"
                    }
                );
                if want_more {
                    self.start_key_scan(tab, false);
                } else if first_batch && count > 0 {
                    self.load_selected_key(tab, false);
                }
            }
            Err(error) => self.notify(Level::Error, error.to_string()),
        }
    }

    pub(crate) fn select_key_row(&mut self, row: usize) {
        let Some(tab) = self.workspace.active_tab_id() else {
            return;
        };
        if let Some(kv) = self.kv_tab(tab) {
            kv.focus = KeyValueFocus::Keys;
            kv.selected_key = row.min(kv.keys.len().saturating_sub(1));
        }
        self.load_selected_key(tab, false);
    }

    /// Describes the selected key and loads the first page of its value, or the next page
    /// when `append` is set.
    pub(crate) fn load_selected_key(&mut self, tab: TabId, append: bool) {
        let request = self.next_request();
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        let Some(key) = kv.selected_key_name().cloned() else {
            return;
        };
        let source = kv.data_source;
        let known_type = kv
            .description
            .as_ref()
            .filter(|description| description.key == key)
            .map(|description| description.key_type.clone());
        let cursor = if append {
            kv.value.as_ref().and_then(ValueContent::next_cursor)
        } else {
            Some(0)
        };
        let Some(cursor) = cursor else {
            self.status = "end of value".into();
            return;
        };
        if !append {
            kv.selected_item = 0;
            kv.item_scroll = 0;
        }
        kv.value_loading = Some(request);
        let Some(connection) = self.connection(source) else {
            return;
        };
        self.spawn(async move {
            let store = match loaders::key_value(connection.as_ref()) {
                Ok(store) => store,
                Err(error) => {
                    return AppEvent::KeyLoaded {
                        tab,
                        request,
                        key,
                        description: None,
                        result: Err(error),
                    };
                }
            };
            let description = match known_type {
                Some(_) if append => None,
                _ => match store.describe_key(&key).await {
                    Ok(description) => Some(description),
                    Err(error) => {
                        return AppEvent::KeyLoaded {
                            tab,
                            request,
                            key,
                            description: None,
                            result: Err(error),
                        };
                    }
                },
            };
            let key_type = description
                .as_ref()
                .map(|description| description.key_type.clone())
                .or(known_type)
                .unwrap_or(KeyType::Missing);
            let count = if key_type == KeyType::String {
                STRING_PREVIEW_BYTES
            } else {
                VALUE_PAGE_SIZE
            };
            let result = if key_type == KeyType::Missing {
                Ok(ValuePage::Unsupported {
                    key_type: "missing (deleted or expired)".into(),
                })
            } else {
                store
                    .read_value(&key, &key_type, ValueRange { cursor, count })
                    .await
            };
            AppEvent::KeyLoaded {
                tab,
                request,
                key,
                description,
                result,
            }
        });
    }

    pub(crate) fn on_key_loaded(
        &mut self,
        tab: TabId,
        request: RequestId,
        key: KeyName,
        description: Option<KeyDescription>,
        result: Result<ValuePage, DriverError>,
    ) {
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        if kv.value_loading != Some(request) {
            return;
        }
        kv.value_loading = None;
        match result {
            Ok(page) => {
                let appending = description.is_none()
                    && kv
                        .description
                        .as_ref()
                        .is_some_and(|known| known.key == key);
                if let Some(description) = description {
                    kv.description = Some(description);
                }
                match (&mut kv.value, appending) {
                    (Some(content), true) => content.append(page),
                    _ => kv.value = Some(ValueContent::from_page(page)),
                }
            }
            Err(error) => self.notify(Level::Error, error.to_string()),
        }
    }

    pub(crate) fn on_server_details(
        &mut self,
        tab: TabId,
        result: Result<Vec<(String, String)>, DriverError>,
    ) {
        match result {
            Ok(details) => {
                if let Some(kv) = self.kv_tab(tab) {
                    kv.server_details = details;
                }
            }
            Err(error) => tracing::warn!(%error, "could not read redis server details"),
        }
    }

    pub(crate) fn key_value_command(&mut self, command: Command) {
        let Some(tab) = self.workspace.active_tab_id() else {
            return;
        };
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        let focus = kv.focus;
        match command {
            Command::MoveUp | Command::MoveDown | Command::PageUp | Command::PageDown | Command::MoveTop | Command::MoveBottom => {
                let delta = match command {
                    Command::MoveUp => -1,
                    Command::MoveDown => 1,
                    Command::PageUp => -KEY_PAGE,
                    Command::PageDown => KEY_PAGE,
                    Command::MoveTop => isize::MIN / 2,
                    _ => isize::MAX / 2,
                };
                self.move_kv_selection(tab, delta);
            }
            Command::MoveRight | Command::Activate if focus == KeyValueFocus::Keys => {
                if let Some(kv) = self.kv_tab(tab) {
                    kv.focus = KeyValueFocus::Value;
                }
            }
            Command::MoveLeft => {
                if let Some(kv) = self.kv_tab(tab) {
                    kv.focus = KeyValueFocus::Keys;
                }
            }
            Command::Filter => {
                if let Some(kv) = self.kv_tab(tab) {
                    kv.focus = KeyValueFocus::Pattern;
                    kv.pattern.move_end();
                }
            }
            Command::FocusConsole => {
                if let Some(kv) = self.kv_tab(tab) {
                    kv.focus = KeyValueFocus::Console;
                }
            }
            Command::Refresh => self.start_key_scan(tab, true),
            Command::LoadMore if focus == KeyValueFocus::Value => self.load_selected_key(tab, true),
            Command::LoadMore => self.start_key_scan(tab, false),
            Command::CreateKey if focus == KeyValueFocus::Value => self.prompt_add_element(tab),
            Command::CreateKey => self.open_prompt(
                "New key",
                "<type> <key> <value…>  e.g. string greeting hello · hash user:1 name ada · zset board 10 alice",
                PromptAction::NewKey { tab },
                "",
            ),
            Command::DeleteKey if focus == KeyValueFocus::Value => self.confirm_delete_element(tab),
            Command::DeleteKey => self.confirm_delete_key(tab),
            Command::EditValue | Command::Activate => self.edit_key_value(tab),
            Command::SetTtl => self.prompt_ttl(tab),
            Command::CopyCell => self.copy_key_value(tab),
            _ => {}
        }
    }

    fn move_kv_selection(&mut self, tab: TabId, delta: isize) {
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        match kv.focus {
            KeyValueFocus::Value => {
                let count = kv.value.as_ref().map_or(0, ValueContent::len);
                kv.selected_item = kv
                    .selected_item
                    .saturating_add_signed(delta)
                    .min(count.saturating_sub(1));
                let reached_end = count > 0
                    && kv.selected_item + 1 == count
                    && kv
                        .value
                        .as_ref()
                        .and_then(ValueContent::next_cursor)
                        .is_some();
                if reached_end && kv.value_loading.is_none() {
                    self.load_selected_key(tab, true);
                }
            }
            _ => {
                let previous = kv.selected_key;
                kv.selected_key = kv
                    .selected_key
                    .saturating_add_signed(delta)
                    .min(kv.keys.len().saturating_sub(1));
                let changed = previous != kv.selected_key;
                let reached_end = kv.selected_key + 1 >= kv.keys.len()
                    && !kv.scan_complete
                    && kv.scanning.is_none();
                if reached_end {
                    self.start_key_scan(tab, false);
                }
                if changed {
                    self.load_selected_key(tab, false);
                }
            }
        }
    }

    pub(crate) fn key_value_raw_key(&mut self, chord: KeyChord) {
        let Some(tab) = self.workspace.active_tab_id() else {
            return;
        };
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        match kv.focus {
            KeyValueFocus::Pattern => match chord.code {
                KeyCode::Enter => {
                    kv.focus = KeyValueFocus::Keys;
                    self.start_key_scan(tab, true);
                }
                KeyCode::Esc => kv.focus = KeyValueFocus::Keys,
                _ => {
                    edit_line(&mut kv.pattern, chord);
                }
            },
            KeyValueFocus::Console => match chord.code {
                KeyCode::Enter => self.submit_console_command(),
                KeyCode::Esc => kv.focus = KeyValueFocus::Keys,
                KeyCode::Up | KeyCode::Down => {
                    let len = kv.console_history.len();
                    if len == 0 {
                        return;
                    }
                    let next = match (kv.history_index, chord.code) {
                        (None, KeyCode::Up) => Some(0),
                        (Some(index), KeyCode::Up) => Some((index + 1).min(len - 1)),
                        (Some(0), _) | (None, _) => None,
                        (Some(index), _) => Some(index - 1),
                    };
                    kv.history_index = next;
                    let text = next
                        .and_then(|index| kv.console_history.get(index))
                        .unwrap_or_default()
                        .to_owned();
                    kv.console_input.set(&text);
                }
                _ => {
                    edit_line(&mut kv.console_input, chord);
                }
            },
            KeyValueFocus::Value | KeyValueFocus::Keys => {
                if chord.code == KeyCode::Esc {
                    kv.focus = KeyValueFocus::Keys;
                }
            }
        }
    }

    pub(crate) fn submit_console_command(&mut self) {
        let Some(tab) = self.workspace.active_tab_id() else {
            return;
        };
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        if kv.focus != KeyValueFocus::Console {
            kv.focus = KeyValueFocus::Console;
            return;
        }
        let line = kv.console_input.text().trim().to_owned();
        if line.is_empty() {
            return;
        }
        let arguments = match split_command_line(&line) {
            Ok(arguments) => arguments,
            Err(error) => {
                kv.push_console(format!("(error) {error}"), true, false);
                return;
            }
        };
        kv.console_history.push(&line);
        kv.history_index = None;
        kv.console_input.clear();
        kv.push_console(format!("> {line}"), false, true);
        let source = kv.data_source;
        let Some(connection) = self.connection(source) else {
            self.notify(Level::Warning, "not connected");
            return;
        };
        self.spawn(async move {
            let result = match loaders::key_value(connection.as_ref()) {
                Ok(store) => store.execute_command(&arguments).await,
                Err(error) => Err(error),
            };
            AppEvent::CommandReplied { tab, result }
        });
    }

    pub(crate) fn on_command_replied(
        &mut self,
        tab: TabId,
        result: Result<CommandReply, DriverError>,
    ) {
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        match result {
            Ok(reply) => {
                let is_error = matches!(reply, CommandReply::Error(_));
                for line in reply.render() {
                    kv.push_console(line, is_error, false);
                }
            }
            Err(error) => kv.push_console(format!("(error) {error}"), true, false),
        }
    }

    fn write_key(&mut self, tab: TabId, operation: KeyValueWrite, rescan: bool) {
        let Some(source) = self.kv_tab(tab).map(|kv| kv.data_source) else {
            return;
        };
        let Some(connection) = self.connection(source) else {
            self.notify(Level::Warning, "not connected");
            return;
        };
        self.spawn(async move {
            let result = match loaders::key_value(connection.as_ref()) {
                Ok(store) => store.write(&operation).await,
                Err(error) => Err(error),
            };
            AppEvent::KeyWritten {
                tab,
                result,
                rescan,
            }
        });
    }

    pub(crate) fn on_key_written(
        &mut self,
        tab: TabId,
        result: Result<(), DriverError>,
        rescan: bool,
    ) {
        match result {
            Ok(()) => {
                self.status = "saved".into();
                if rescan {
                    self.start_key_scan(tab, true);
                } else {
                    self.load_selected_key(tab, false);
                }
            }
            Err(error) => self.notify(Level::Error, error.to_string()),
        }
    }

    fn confirm_delete_key(&mut self, tab: TabId) {
        let Some(key) = self
            .kv_tab(tab)
            .and_then(|kv| kv.selected_key_name().cloned())
        else {
            return;
        };
        self.overlays.push(Overlay::Confirm(ConfirmState {
            title: "Delete key?".into(),
            lines: vec![format!("DEL {}", key.display())],
            confirm_label: "Delete".into(),
            danger: true,
            action: PendingAction::DeleteKey { tab, key },
        }));
    }

    pub(crate) fn delete_key(&mut self, tab: TabId, key: KeyName) {
        self.write_key(tab, KeyValueWrite::Delete { key }, true);
    }

    fn confirm_delete_element(&mut self, tab: TabId) {
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        let Some(item) = kv.selected_item().cloned() else {
            return;
        };
        if kv.key_type() == Some(&KeyType::List) || kv.key_type() == Some(&KeyType::Stream) {
            self.notify(Level::Info, "removing single list/stream elements is not supported; use the console (LREM/XDEL)");
            return;
        }
        let label = item.field.as_ref().map_or_else(
            || String::from_utf8_lossy(&item.value).into_owned(),
            |field| String::from_utf8_lossy(field).into_owned(),
        );
        self.overlays.push(Overlay::Confirm(ConfirmState {
            title: "Remove element?".into(),
            lines: vec![label],
            confirm_label: "Remove".into(),
            danger: true,
            action: PendingAction::DeleteElement { tab },
        }));
    }

    pub(crate) fn delete_element(&mut self, tab: TabId) {
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        let (Some(key), Some(item), Some(key_type)) = (
            kv.selected_key_name().cloned(),
            kv.selected_item().cloned(),
            kv.key_type().cloned(),
        ) else {
            return;
        };
        let operation = match key_type {
            KeyType::Hash => item
                .field
                .map(|field| KeyValueWrite::HashDelete { key, field }),
            KeyType::Set => Some(KeyValueWrite::SetRemove {
                key,
                member: item.value,
            }),
            KeyType::SortedSet => Some(KeyValueWrite::SortedSetRemove {
                key,
                member: item.value,
            }),
            _ => None,
        };
        if let Some(operation) = operation {
            self.write_key(tab, operation, false);
        }
    }

    fn prompt_ttl(&mut self, tab: TabId) {
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        let Some(key) = kv.selected_key_name().cloned() else {
            return;
        };
        self.overlays.push(Overlay::Prompt(PromptState {
            title: format!("TTL for {}", key.display()),
            hint: "seconds until expiry; empty or 0 removes the expiry".into(),
            input: LineInput::default(),
            masked: false,
            action: PromptAction::SetTtl { tab, key },
        }));
    }

    pub(crate) fn set_ttl(&mut self, tab: TabId, key: KeyName, text: &str) {
        let text = text.trim();
        let operation = if text.is_empty() || text == "0" {
            KeyValueWrite::Persist { key }
        } else {
            match text.parse::<u64>() {
                Ok(seconds) => KeyValueWrite::Expire { key, seconds },
                Err(_) => {
                    self.notify(Level::Error, "TTL must be a whole number of seconds");
                    return;
                }
            }
        };
        self.write_key(tab, operation, false);
    }

    fn edit_key_value(&mut self, tab: TabId) {
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        let Some(key) = kv.selected_key_name().cloned() else {
            return;
        };
        let (title, text, action) = match (&kv.value, kv.focus) {
            (
                Some(ValueContent::Text {
                    bytes,
                    total_length,
                }),
                _,
            ) => {
                if bytes.len() as u64 != *total_length {
                    self.notify(
                        Level::Info,
                        "value is too large to edit inline; use the console",
                    );
                    return;
                }
                (
                    format!("Edit {}", key.display()),
                    String::from_utf8_lossy(bytes).into_owned(),
                    TextEditAction::RedisString { tab, key },
                )
            }
            (Some(ValueContent::Items { .. }), KeyValueFocus::Value) => {
                let Some(item) = kv.selected_item() else {
                    return;
                };
                let text = match kv.key_type() {
                    Some(KeyType::SortedSet) => item
                        .score
                        .map(|score| score.to_string())
                        .unwrap_or_default(),
                    _ => String::from_utf8_lossy(&item.value).into_owned(),
                };
                (
                    format!("Edit element of {}", key.display()),
                    text,
                    TextEditAction::RedisElement { tab },
                )
            }
            (Some(ValueContent::Items { .. }), _) => {
                kv.focus = KeyValueFocus::Value;
                return;
            }
            _ => return,
        };
        self.overlays
            .push(Overlay::TextEdit(Box::new(TextEditState {
                title,
                buffer: TextBuffer::from_text(&text),
                scroll: 0,
                action,
            })));
    }

    pub(crate) fn save_key_value_edit(&mut self, action: TextEditAction, text: String) {
        match action {
            TextEditAction::RedisString { tab, key } => {
                let ttl = self
                    .kv_tab(tab)
                    .and_then(|kv| kv.description.as_ref())
                    .and_then(|description| match description.ttl {
                        relix_core::keyvalue::Ttl::Seconds(seconds) => u64::try_from(seconds).ok(),
                        _ => None,
                    });
                self.write_key(
                    tab,
                    KeyValueWrite::SetString {
                        key,
                        value: text.into_bytes(),
                        ttl_seconds: ttl,
                    },
                    false,
                );
            }
            TextEditAction::RedisElement { tab } => self.save_element_edit(tab, text),
            TextEditAction::GridCell { .. } => {}
        }
    }

    fn save_element_edit(&mut self, tab: TabId, text: String) {
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        let (Some(key), Some(item), Some(key_type)) = (
            kv.selected_key_name().cloned(),
            kv.selected_item().cloned(),
            kv.key_type().cloned(),
        ) else {
            return;
        };
        let operation = match key_type {
            KeyType::Hash => item.field.map(|field| KeyValueWrite::HashSet {
                key,
                field,
                value: text.into_bytes(),
            }),
            KeyType::List => item
                .index
                .and_then(|index| i64::try_from(index).ok())
                .map(|index| KeyValueWrite::ListSet {
                    key,
                    index,
                    value: text.into_bytes(),
                }),
            KeyType::SortedSet => match text.trim().parse::<f64>() {
                Ok(score) => Some(KeyValueWrite::SortedSetAdd {
                    key,
                    member: item.value,
                    score,
                }),
                Err(_) => {
                    self.notify(Level::Error, "score must be a number");
                    return;
                }
            },
            KeyType::Set => {
                self.write_key(
                    tab,
                    KeyValueWrite::SetRemove {
                        key: key.clone(),
                        member: item.value,
                    },
                    false,
                );
                Some(KeyValueWrite::SetAdd {
                    key,
                    member: text.into_bytes(),
                })
            }
            _ => None,
        };
        match operation {
            Some(operation) => self.write_key(tab, operation, false),
            None => self.notify(Level::Info, "this element type cannot be edited inline"),
        }
    }

    fn prompt_add_element(&mut self, tab: TabId) {
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        let hint = match kv.key_type() {
            Some(KeyType::Hash) => "<field> <value…>",
            Some(KeyType::List) => "<value…> (appended to the tail)",
            Some(KeyType::Set) => "<member…>",
            Some(KeyType::SortedSet) => "<score> <member…>",
            _ => {
                self.notify(Level::Info, "select a hash, list, set or sorted set");
                return;
            }
        };
        self.open_prompt("Add element", hint, PromptAction::AddElement { tab }, "");
    }

    pub(crate) fn add_element(&mut self, tab: TabId, text: &str) {
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        let (Some(key), Some(key_type)) = (kv.selected_key_name().cloned(), kv.key_type().cloned())
        else {
            return;
        };
        match parse_element(&key_type, key, text) {
            Ok(operation) => self.write_key(tab, operation, false),
            Err(error) => self.notify(Level::Error, error),
        }
    }

    pub(crate) fn create_key(&mut self, tab: TabId, text: &str) {
        match parse_new_key(text) {
            Ok(operation) => self.write_key(tab, operation, true),
            Err(error) => self.notify(Level::Error, error),
        }
    }

    fn copy_key_value(&mut self, tab: TabId) {
        let Some(kv) = self.kv_tab(tab) else {
            return;
        };
        let text = match (&kv.value, kv.focus) {
            (Some(ValueContent::Text { bytes, .. }), _) => {
                Some(String::from_utf8_lossy(bytes).into_owned())
            }
            (_, KeyValueFocus::Value) => kv
                .selected_item()
                .map(|item| String::from_utf8_lossy(&item.value).into_owned()),
            _ => kv.selected_key_name().map(KeyName::display),
        };
        if let Some(text) = text {
            self.copy_to_clipboard(&text);
        }
    }
}

/// Splits a console line into arguments like `redis-cli`: whitespace separated, with
/// single or double quotes grouping words and backslash escapes inside double quotes.
pub fn split_command_line(line: &str) -> Result<Vec<String>, String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut has_token = false;
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some('"'), '\\') => {
                if let Some(next) = chars.next() {
                    current.push(match next {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                }
            }
            (Some(open), ch) if ch == open => quote = None,
            (Some(_), ch) => current.push(ch),
            (None, '"' | '\'') => {
                quote = Some(ch);
                has_token = true;
            }
            (None, ch) if ch.is_whitespace() => {
                if has_token {
                    arguments.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            (None, ch) => {
                current.push(ch);
                has_token = true;
            }
        }
    }
    if quote.is_some() {
        return Err("unbalanced quotes".into());
    }
    if has_token {
        arguments.push(current);
    }
    Ok(arguments)
}

fn parse_new_key(text: &str) -> Result<KeyValueWrite, String> {
    let words = split_command_line(text)?;
    let [kind, key, rest @ ..] = words.as_slice() else {
        return Err("expected: <type> <key> <value…>".into());
    };
    let key_type = match kind.to_lowercase().as_str() {
        "string" => KeyType::String,
        "hash" => KeyType::Hash,
        "list" => KeyType::List,
        "set" => KeyType::Set,
        "zset" => KeyType::SortedSet,
        other => return Err(format!("unknown key type '{other}'")),
    };
    parse_element(&key_type, KeyName(key.as_bytes().to_vec()), &rest.join(" "))
}

fn parse_element(key_type: &KeyType, key: KeyName, text: &str) -> Result<KeyValueWrite, String> {
    let words = split_command_line(text)?;
    let joined = |words: &[String]| words.join(" ").into_bytes();
    match (key_type, words.as_slice()) {
        (KeyType::String, _) => Ok(KeyValueWrite::SetString {
            key,
            value: joined(&words),
            ttl_seconds: None,
        }),
        (KeyType::Hash, [field, value @ ..]) if !value.is_empty() => Ok(KeyValueWrite::HashSet {
            key,
            field: field.as_bytes().to_vec(),
            value: joined(value),
        }),
        (KeyType::List, words) if !words.is_empty() => Ok(KeyValueWrite::ListPush {
            key,
            value: joined(words),
            tail: true,
        }),
        (KeyType::Set, words) if !words.is_empty() => Ok(KeyValueWrite::SetAdd {
            key,
            member: joined(words),
        }),
        (KeyType::SortedSet, [score, member @ ..]) if !member.is_empty() => {
            let score = score
                .parse::<f64>()
                .map_err(|_| format!("invalid score '{score}'"))?;
            Ok(KeyValueWrite::SortedSetAdd {
                key,
                member: joined(member),
                score,
            })
        }
        (KeyType::Hash, _) => Err("expected: <field> <value…>".into()),
        (KeyType::SortedSet, _) => Err("expected: <score> <member…>".into()),
        _ => Err("a value is required".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_redis_cli() {
        assert_eq!(
            split_command_line(r#"SET k "a b\n" 'c d'"#).unwrap(),
            vec!["SET", "k", "a b\n", "c d"]
        );
        assert_eq!(split_command_line("GET \"\"").unwrap(), vec!["GET", ""]);
        assert!(split_command_line("GET \"open").is_err());
    }

    #[test]
    fn parses_new_keys_per_type() {
        assert_eq!(
            parse_new_key("hash user:1 name ada lovelace").unwrap(),
            KeyValueWrite::HashSet {
                key: KeyName::from("user:1"),
                field: b"name".to_vec(),
                value: b"ada lovelace".to_vec()
            }
        );
        assert_eq!(
            parse_new_key("zset board 10 alice").unwrap(),
            KeyValueWrite::SortedSetAdd {
                key: KeyName::from("board"),
                member: b"alice".to_vec(),
                score: 10.0
            }
        );
        assert!(parse_new_key("zset board ten alice").is_err());
        assert!(parse_new_key("blob x y").is_err());
        assert!(parse_new_key("hash only-key").is_err());
    }
}
