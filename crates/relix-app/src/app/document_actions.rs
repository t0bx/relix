/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use super::connections::Deferred;
use super::explorer_actions::edit_line;
use crate::command::Command;
use crate::document_tab::{DocumentFocus, DocumentTab};
use crate::events::AppEvent;
use crate::loaders;
use crate::notifications::Level;
use crate::overlay::{
    ConfirmState, Overlay, PendingAction, PickerItem, PickerPurpose, PickerState, PickerValue,
    TextEditAction, TextEditState, ViewerState,
};
use crate::requests::RequestId;
use crate::tab::Tab;
use crate::workspace::TabId;
use relix_config::{KeyChord, KeyCode};
use relix_core::{
    CollectionDetails, CollectionKind, CollectionRef, DataSourceId, DocumentPage, DocumentWrite,
    DocumentWriteOutcome,
};
use relix_driver_api::DriverError;
use serde_json::Value as Json;

const DOCUMENT_PAGE_STEP: isize = 15;

impl App {
    pub(crate) fn open_document_tab(
        &mut self,
        source: DataSourceId,
        collection: CollectionRef,
        kind: CollectionKind,
        new_pane: bool,
    ) {
        let existing = self.workspace.tabs().find_map(|(id, tab)| match tab {
            Tab::Document(tab) if tab.data_source == source && tab.collection == collection => {
                Some(id)
            }
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
        let tab = Tab::Document(Box::new(DocumentTab::new(source, collection, kind)));
        let id = self.workspace.add_tab(pane, tab);
        self.focus_workspace();
        self.load_documents(id);
    }

    fn document_tab(&mut self, tab: TabId) -> Option<&mut DocumentTab> {
        self.workspace.tab_mut(tab).and_then(Tab::as_document_mut)
    }

    pub(crate) fn load_documents(&mut self, tab: TabId) {
        let request = self.next_request();
        let Some(document) = self.document_tab(tab) else {
            return;
        };
        let source = document.data_source;
        let collection = document.collection.clone();
        let query = match document.query() {
            Ok(query) => query,
            Err(error) => {
                document.error = Some(error);
                return;
            }
        };
        document.loading = Some(request);
        document.error = None;
        let Some(connection) = self.connection_or_defer(source, Deferred::LoadDocuments(tab))
        else {
            return;
        };
        if let Some(store) = connection.as_document()
            && let Some(document) = self.document_tab(tab)
        {
            document.filter_hint = store.filter_hint().to_owned();
        }
        self.spawn(async move {
            let result = match loaders::document(connection.as_ref()) {
                Ok(store) => store.find(&collection, &query).await,
                Err(error) => Err(error),
            };
            AppEvent::DocumentsLoaded {
                tab,
                request,
                result,
            }
        });
    }

    pub(crate) fn on_documents_loaded(
        &mut self,
        tab: TabId,
        request: RequestId,
        result: Result<DocumentPage, DriverError>,
    ) {
        let Some(document) = self.document_tab(tab) else {
            return;
        };
        if document.loading != Some(request) {
            return;
        }
        document.loading = None;
        let source = document.data_source;
        match result {
            Ok(page) => {
                document.documents = page.documents;
                document.next_page = page.next_page;
                document.total = page.total.or(document.total);
                document.selected = document
                    .selected
                    .min(document.documents.len().saturating_sub(1));
            }
            Err(error) => {
                document.error = Some(error.to_string());
                self.note_driver_error(source, &error);
            }
        }
    }

    pub(crate) fn document_command(&mut self, command: Command) {
        let Some(tab) = self.workspace.active_tab_id() else {
            return;
        };
        let Some(document) = self.document_tab(tab) else {
            return;
        };
        match command {
            Command::MoveUp => document.move_selection(-1),
            Command::MoveDown => document.move_selection(1),
            Command::PageUp => document.move_selection(-DOCUMENT_PAGE_STEP),
            Command::PageDown => document.move_selection(DOCUMENT_PAGE_STEP),
            Command::MoveTop => document.move_selection(isize::MIN / 2),
            Command::MoveBottom => document.move_selection(isize::MAX / 2),
            Command::MoveRight => document.detail_scroll += 1,
            Command::MoveLeft => document.detail_scroll = document.detail_scroll.saturating_sub(1),
            Command::Filter => {
                document.focus = DocumentFocus::Filter;
                document.filter.move_end();
            }
            Command::NextPage => {
                if document.go_next() {
                    self.load_documents(tab);
                }
            }
            Command::PreviousPage => {
                if document.go_previous() {
                    self.load_documents(tab);
                }
            }
            Command::Refresh => self.load_documents(tab),
            Command::CreateKey => self.edit_document(tab, true),
            Command::EditValue | Command::Activate => self.edit_document(tab, false),
            Command::DeleteKey => self.confirm_delete_document(tab),
            Command::CopyCell => {
                if let Some(json) = document.selected_json() {
                    self.copy_to_clipboard(&json);
                }
            }
            Command::InspectObject => self.describe_collection(tab, false),
            Command::CreateIndex => self.edit_new_index(tab),
            Command::DropIndex => self.describe_collection(tab, true),
            _ => {}
        }
    }

    pub(crate) fn document_raw_key(&mut self, chord: KeyChord) {
        let Some(tab) = self.workspace.active_tab_id() else {
            return;
        };
        let Some(document) = self.document_tab(tab) else {
            return;
        };
        if document.focus != DocumentFocus::Filter {
            return;
        }
        match chord.code {
            KeyCode::Enter => {
                document.focus = DocumentFocus::List;
                document.reset_paging();
                self.load_documents(tab);
            }
            KeyCode::Esc => document.focus = DocumentFocus::List,
            _ => {
                edit_line(&mut document.filter, chord);
            }
        }
    }

    fn edit_document(&mut self, tab: TabId, insert: bool) {
        let Some(document) = self.document_tab(tab) else {
            return;
        };
        if !document.kind.is_writable() {
            self.notify(Level::Info, "this collection is read-only");
            return;
        }
        let name = document.collection.to_string();
        let (title, text, key) = if insert {
            (format!("Insert into {name}"), "{\n  \n}".to_owned(), None)
        } else {
            let Some(selected) = document.selected_document() else {
                return;
            };
            let text = serde_json::to_string_pretty(&selected.body).unwrap_or_default();
            (
                format!("Edit {} in {name}", selected.key_label()),
                text,
                Some(selected.key.clone()),
            )
        };
        self.overlays
            .push(Overlay::TextEdit(Box::new(TextEditState::new(
                format!("{title} · Ctrl+S save"),
                &text,
                TextEditAction::Document { tab, key },
            ))));
    }

    fn edit_new_index(&mut self, tab: TabId) {
        let Some(document) = self.document_tab(tab) else {
            return;
        };
        let title = format!("New index on {} · Ctrl+S create", document.collection);
        self.overlays
            .push(Overlay::TextEdit(Box::new(TextEditState::new(
                title,
                "{\n  \n}",
                TextEditAction::DocumentIndex { tab },
            ))));
    }

    /// Parses an edited document and writes it. Invalid JSON reopens the editor with the
    /// text unchanged, so a typo never loses the user's edits.
    pub(crate) fn save_document_edit(&mut self, tab: TabId, key: Option<Json>, text: String) {
        let parsed = serde_json::from_str::<Json>(&text);
        let document = match parsed {
            Ok(document) if document.is_object() => document,
            Ok(_) | Err(_) => {
                let reason = parsed.err().map_or_else(
                    || "a document must be a JSON object".to_owned(),
                    |error| error.to_string(),
                );
                self.notify(Level::Error, format!("invalid document: {reason}"));
                self.overlays
                    .push(Overlay::TextEdit(Box::new(TextEditState::new(
                        "Fix the document · Ctrl+S save".into(),
                        &text,
                        TextEditAction::Document { tab, key },
                    ))));
                return;
            }
        };
        let write = match key {
            Some(key) => DocumentWrite::Replace { key, document },
            None => DocumentWrite::Insert { document },
        };
        self.write_document(tab, write);
    }

    pub(crate) fn save_index_definition(&mut self, tab: TabId, text: String) {
        match serde_json::from_str::<Json>(&text) {
            Ok(definition) => self.write_document(tab, DocumentWrite::CreateIndex { definition }),
            Err(error) => {
                self.notify(Level::Error, format!("invalid index definition: {error}"));
                self.overlays
                    .push(Overlay::TextEdit(Box::new(TextEditState::new(
                        "Fix the index definition · Ctrl+S create".into(),
                        &text,
                        TextEditAction::DocumentIndex { tab },
                    ))));
            }
        }
    }

    fn confirm_delete_document(&mut self, tab: TabId) {
        let Some(document) = self.document_tab(tab) else {
            return;
        };
        let Some(selected) = document.selected_document() else {
            return;
        };
        let label = selected.key_label();
        let key = selected.key.clone();
        let collection = document.collection.to_string();
        self.overlays.push(Overlay::Confirm(ConfirmState {
            title: format!("Delete document {label}?"),
            lines: vec![format!("The document is removed from {collection}.")],
            confirm_label: "Delete".into(),
            danger: true,
            action: PendingAction::DeleteDocument { tab, key },
        }));
    }

    pub(crate) fn delete_document(&mut self, tab: TabId, key: Json) {
        self.write_document(tab, DocumentWrite::Delete { key });
    }

    pub(crate) fn drop_document_index(&mut self, tab: TabId, name: String) {
        self.write_document(tab, DocumentWrite::DropIndex { name });
    }

    fn write_document(&mut self, tab: TabId, write: DocumentWrite) {
        let Some(document) = self.document_tab(tab) else {
            return;
        };
        let source = document.data_source;
        let collection = document.collection.clone();
        let Some(connection) = self.connection(source) else {
            self.notify(Level::Error, "not connected");
            return;
        };
        self.spawn(async move {
            let result = match loaders::document(connection.as_ref()) {
                Ok(store) => store.write(&collection, &write).await,
                Err(error) => Err(error),
            };
            AppEvent::DocumentWritten { tab, result }
        });
    }

    pub(crate) fn on_document_written(
        &mut self,
        tab: TabId,
        result: Result<DocumentWriteOutcome, DriverError>,
    ) {
        match result {
            Ok(outcome) => {
                let message = outcome
                    .message
                    .unwrap_or_else(|| format!("{} document(s) affected", outcome.affected));
                self.notify(Level::Success, message);
                self.load_documents(tab);
            }
            Err(error) => {
                if let Some(source) = self.workspace.tab(tab).and_then(Tab::data_source) {
                    self.note_driver_error(source, &error);
                }
                self.notify(Level::Error, error.to_string());
            }
        }
    }

    fn describe_collection(&mut self, tab: TabId, choose_index_to_drop: bool) {
        let Some(document) = self.document_tab(tab) else {
            return;
        };
        let source = document.data_source;
        let collection = document.collection.clone();
        let Some(connection) = self.connection(source) else {
            self.notify(Level::Error, "not connected");
            return;
        };
        self.spawn(async move {
            let result = match loaders::document(connection.as_ref()) {
                Ok(store) => store.describe_collection(&collection).await,
                Err(error) => Err(error),
            };
            AppEvent::CollectionDescribed {
                tab,
                choose_index_to_drop,
                result,
            }
        });
    }

    pub(crate) fn on_collection_described(
        &mut self,
        tab: TabId,
        choose_index_to_drop: bool,
        result: Result<CollectionDetails, DriverError>,
    ) {
        let details = match result {
            Ok(details) => details,
            Err(error) => {
                self.notify(Level::Error, error.to_string());
                return;
            }
        };
        if choose_index_to_drop {
            let items = details
                .indexes
                .iter()
                .map(|index| PickerItem {
                    label: index.name.clone(),
                    detail: index.definition.to_string(),
                    hint: if index.unique {
                        "unique".into()
                    } else {
                        String::new()
                    },
                    value: PickerValue::DocumentIndex {
                        tab,
                        name: index.name.clone(),
                    },
                })
                .collect();
            self.overlays.push(Overlay::Picker(PickerState::new(
                "Drop index",
                PickerPurpose::DocumentIndexes,
                items,
                "Enter choose · Esc cancel",
            )));
        } else {
            let text = describe_details(&details);
            self.overlays.push(Overlay::Viewer(ViewerState::new(
                &format!("{} details", details.collection),
                &text,
            )));
        }
        if let Some(document) = self.document_tab(tab) {
            document.details = Some(details);
        }
    }

    pub(crate) fn confirm_drop_document_index(&mut self, tab: TabId, name: String) {
        self.overlays.push(Overlay::Confirm(ConfirmState {
            title: format!("Drop index '{name}'?"),
            lines: vec!["Queries that rely on it become slower; data is kept unless the index is the collection itself.".into()],
            confirm_label: "Drop".into(),
            danger: true,
            action: PendingAction::DropDocumentIndex { tab, name },
        }));
    }
}

fn describe_details(details: &CollectionDetails) -> String {
    let mut lines = vec![
        format!("{} ({})", details.collection, details.kind.label()),
        String::new(),
    ];
    if !details.key_fields.is_empty() {
        lines.push(format!("key: {}", details.key_fields.join(", ")));
    }
    for (name, value) in &details.properties {
        lines.push(format!("{name}: {value}"));
    }
    lines.push(String::new());
    lines.push(format!("indexes ({})", details.indexes.len()));
    for index in &details.indexes {
        let unique = if index.unique { " unique" } else { "" };
        lines.push(format!("  {}{unique}: {}", index.name, index.definition));
        if let Some(detail) = &index.detail {
            lines.push(format!("    {detail}"));
        }
    }
    if let Some(schema) = &details.schema {
        lines.push(String::new());
        lines.push("schema".into());
        lines.push(serde_json::to_string_pretty(schema).unwrap_or_default());
    }
    lines.join("\n")
}
