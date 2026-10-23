/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::Focus;
use crate::command::Command;
use crate::overlay::Overlay;
use crate::tab::Tab;
use crate::test_support::AppHarness as Harness;
use relix_config::{KeyChord, KeyCode};
use relix_core::{DataSourceDefinition, DriverId};
use relix_testing::mock_document::MockDocumentDriver;
use serde_json::json;
use std::sync::Arc;

fn orders() -> Vec<serde_json::Value> {
    (1..=60)
        .map(|id| json!({"_id": id, "status": if id % 3 == 0 { "open" } else { "closed" }, "total": id * 10}))
        .collect()
}

fn document_count(harness: &Harness) -> usize {
    match harness.app.active_tab() {
        Some(Tab::Document(tab)) => tab.documents.len(),
        _ => panic!("document tab expected"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn collections_are_browsed_paged_filtered_and_edited() {
    let dir = tempfile::tempdir().unwrap();
    let store = MockDocumentDriver::with_collection("orders", orders());
    let mut harness = Harness::new(dir.path(), vec![Arc::new(store.clone())]);
    harness.app.startup(Some("Shop"));
    let source = harness.add_data_source(DataSourceDefinition::new(
        "Mongo",
        DriverId::from("mock-doc"),
    ));

    harness.app.focus = Focus::Sidebar;
    let root = harness.app.explorer.root_for(source).unwrap();
    harness.app.expand_node(root);
    harness.settle().await;
    let namespace = harness
        .app
        .explorer
        .find_key(&format!("{source}/ns:app"))
        .unwrap();
    harness.app.expand_node(namespace);
    harness.settle().await;
    let collection = harness
        .app
        .explorer
        .find_key(&format!("{source}/ns:app/orders"))
        .unwrap();
    assert_eq!(
        harness
            .app
            .explorer
            .node(collection)
            .unwrap()
            .detail
            .as_deref(),
        Some("collection · 60")
    );
    harness.app.explorer.select_node(collection);
    harness.app.explorer_command(Command::Activate);
    harness.settle().await;
    assert_eq!(document_count(&harness), 50);

    harness.keys("]");
    harness.settle().await;
    assert_eq!(document_count(&harness), 10);
    harness.keys("[");
    harness.settle().await;
    assert_eq!(document_count(&harness), 50);

    harness.keys("/");
    harness.type_text("{\"status\": \"open\"}\n");
    harness.settle().await;
    assert_eq!(document_count(&harness), 20);

    harness.keys("a");
    let Some(Overlay::TextEdit(_)) = harness.app.overlays.last() else {
        panic!("insert opens the JSON editor");
    };
    harness.app.overlays.clear();
    harness.app.save_document_edit(
        harness.app.workspace.active_tab_id().unwrap(),
        None,
        "{\"_id\": 99, \"status\": \"open\"}".into(),
    );
    harness.settle().await;
    assert_eq!(store.documents("orders").len(), 61);
    assert_eq!(document_count(&harness), 21);

    let tab = harness.app.workspace.active_tab_id().unwrap();
    harness
        .app
        .save_document_edit(tab, None, "[not an object]".into());
    assert!(
        matches!(harness.app.overlays.last(), Some(Overlay::TextEdit(_))),
        "invalid JSON keeps the editor open"
    );
    harness.app.overlays.clear();

    harness.app.save_document_edit(
        tab,
        Some(json!(3)),
        "{\"_id\": 3, \"status\": \"open\", \"total\": 1}".into(),
    );
    harness.settle().await;
    assert!(
        store
            .documents("orders")
            .contains(&json!({"_id": 3, "status": "open", "total": 1}))
    );

    harness.keys("d");
    harness.key(KeyChord::plain(KeyCode::Enter));
    harness.settle().await;
    assert_eq!(store.documents("orders").len(), 60);

    harness.app.save_session().unwrap();
    let mut restored = Harness::new(dir.path(), vec![Arc::new(store)]);
    restored.app.startup(Some("Shop"));
    let restored_tab = restored
        .app
        .workspace
        .tabs()
        .find_map(|(_, tab)| match tab {
            Tab::Document(tab) => Some(tab.filter.text().to_owned()),
            _ => None,
        });
    assert_eq!(restored_tab.as_deref(), Some("{\"status\": \"open\"}"));
}
