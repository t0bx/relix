/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::{ConnectionStatus, Focus};
use crate::command::Command;
use crate::editor_tab::ResultKind;
use crate::explorer::NodeKind;
use crate::grid::GridView;
use crate::keyvalue_tab::ValueContent;
use crate::overlay::Overlay;
use crate::tab::Tab;
use crate::test_support::AppHarness as Harness;
use relix_config::{KeyChord, KeyCode, Modifiers};
use relix_core::{DataSourceDefinition, DriverId, Environment, Value};
use relix_testing::mock_key_value::{MockKeyValueDriver, MockValue};
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
async fn startup_creates_development_project_and_restores_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = Harness::new(dir.path(), vec![Arc::new(MockKeyValueDriver::default())]);
    harness.app.startup(Some("Development"));
    assert_eq!(harness.app.project_name(), Some("Development"));
    harness.keys("ctrl+n");
    harness.type_text("select 1;");
    harness.keys("alt+v");
    assert_eq!(harness.app.workspace.pane_order().len(), 2);
    harness.app.save_session().unwrap();

    let mut restored = Harness::new(dir.path(), vec![Arc::new(MockKeyValueDriver::default())]);
    restored.app.startup(None);
    assert_eq!(restored.app.project_name(), Some("Development"));
    assert_eq!(restored.app.workspace.pane_order().len(), 2);
    let texts: Vec<String> = restored
        .app
        .workspace
        .tabs()
        .filter_map(|(_, tab)| match tab {
            Tab::Editor(editor) => Some(editor.buffer.text()),
            _ => None,
        })
        .collect();
    assert!(
        texts.contains(&"select 1;".to_owned()),
        "draft restored: {texts:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sqlite_end_to_end_browse_query_edit_and_commit() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("notes.db");
    let mut harness = Harness::new(dir.path(), vec![Arc::new(MockKeyValueDriver::default())]);
    harness.app.startup(Some("Development"));
    let source = harness.add_sqlite("Notes", &db, Environment::Development);
    harness.run_sql(source,
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, status TEXT);\n\
         INSERT INTO users (name, status) VALUES ('ada', 'active'), ('bob', 'inactive'), ('cy', 'active');",
    )
    .await;
    assert_eq!(
        harness.app.connection_status(source),
        ConnectionStatus::Connected
    );
    let editor = harness.active_editor();
    assert_eq!(editor.results.len(), 2, "both statements ran");
    assert!(matches!(editor.results[1].kind, ResultKind::Affected(3)));

    harness.app.focus = Focus::Sidebar;
    let root = harness.app.explorer.root_for(source).unwrap();
    harness.app.explorer.select_node(root);
    harness.app.refresh_explorer_roots();
    harness.app.explorer_command(Command::Refresh);
    harness.settle().await;
    let visible: Vec<String> = harness
        .app
        .explorer
        .visible()
        .iter()
        .map(|id| harness.app.explorer.node(*id).unwrap().label.clone())
        .collect();
    assert!(visible.contains(&"main".to_owned()), "{visible:?}");
    let main = harness
        .app
        .explorer
        .find_key(&format!("{source}/db:main"))
        .unwrap();
    harness.app.expand_node(main);
    harness.settle().await;
    let tables = harness
        .app
        .explorer
        .find_key(&format!("{source}/db:main/tables"))
        .unwrap();
    harness.app.expand_node(tables);
    let users = harness
        .app
        .explorer
        .find_key(&format!("{source}/db:main/tables/users"))
        .unwrap();
    harness.app.expand_node(users);
    harness.settle().await;
    let columns = harness
        .app
        .explorer
        .find_key(&format!("{source}/db:main/tables/users/columns/name"))
        .unwrap();
    assert!(matches!(
        harness.app.explorer.node(columns).unwrap().kind,
        NodeKind::Column(_)
    ));

    harness.app.explorer.select_node(users);
    harness.app.explorer_command(Command::Activate);
    harness.settle().await;
    let Some(Tab::Table(table)) = harness.app.active_tab() else {
        panic!("table tab expected");
    };
    assert_eq!(table.grid.as_ref().unwrap().rows.len(), 3);

    harness.keys("w");
    harness.type_text("status = 'active'");
    harness.key(KeyChord::plain(KeyCode::Esc));
    harness.keys("w");
    harness.key(KeyChord::plain(KeyCode::Enter));
    harness.settle().await;
    let Some(Tab::Table(table)) = harness.app.active_tab() else {
        panic!("table tab expected");
    };
    assert_eq!(table.grid.as_ref().unwrap().rows.len(), 2, "filter applied");

    harness.keys("l");
    harness.keys("S");
    harness.settle().await;
    harness.keys("S");
    harness.settle().await;
    let Some(Tab::Table(table)) = harness.app.active_tab() else {
        panic!("table tab expected");
    };
    let names: Vec<Value> = table
        .grid
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .map(|row| row.values[1].clone())
        .collect();
    assert_eq!(
        names,
        vec![Value::Text("cy".into()), Value::Text("ada".into())],
        "sorted descending"
    );

    harness.keys("alt+e");
    assert!(matches!(
        harness.app.overlays.last(),
        Some(Overlay::TextEdit(_))
    ));
    harness.keys("ctrl+a");
    harness.key(KeyChord::plain(KeyCode::Backspace));
    harness.type_text("cyrus");
    harness.keys("ctrl+s");
    harness.keys("ctrl+s");
    assert!(
        matches!(harness.app.overlays.last(), Some(Overlay::Confirm(_))),
        "review dialog shown"
    );
    harness.key(KeyChord::plain(KeyCode::Enter));
    harness.settle().await;

    harness
        .run_sql(source, "SELECT name FROM users ORDER BY id;")
        .await;
    let editor = harness.active_editor();
    let ResultKind::Rows { grid, .. } = &mut editor.results[0].kind else {
        panic!("rows expected");
    };
    assert_eq!(grid.rows[2].values[0], Value::Text("cyrus".into()));
    grid.view = GridView::Json;
    assert!(grid.json_text().contains("\"name\": \"cyrus\""));
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_uses_live_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("shop.db");
    let mut harness = Harness::new(dir.path(), vec![Arc::new(MockKeyValueDriver::default())]);
    harness.app.startup(Some("Development"));
    let source = harness.add_sqlite("Shop", &db, Environment::None);
    harness
        .run_sql(
            source,
            "CREATE TABLE orders (id INTEGER PRIMARY KEY, total REAL);",
        )
        .await;
    harness.app.load_catalog(source);
    harness.settle().await;
    harness.app.new_editor_tab(Some(source), "");
    harness.type_text("select * from ord");
    harness.settle().await;
    let editor = harness.active_editor();
    let popup = editor.completion.as_ref().expect("completion popup");
    assert_eq!(popup.items[0].label, "orders");
    harness.key(KeyChord::plain(KeyCode::Tab));
    harness.type_text(" o where o.");
    harness.settle().await;
    let editor = harness.active_editor();
    assert_eq!(editor.buffer.text(), "select * from orders o where o.");
    let labels: Vec<String> = editor
        .completion
        .as_ref()
        .expect("columns")
        .items
        .iter()
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(labels, vec!["id", "total"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn destructive_statements_need_confirmation_on_production() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("prod.db");
    let mut harness = Harness::new(dir.path(), vec![Arc::new(MockKeyValueDriver::default())]);
    harness.app.startup(Some("Production"));
    let source = harness.add_sqlite("Main", &db, Environment::Production);
    harness
        .run_sql(
            source,
            "CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);",
        )
        .await;
    harness.app.new_editor_tab(Some(source), "DELETE FROM t");
    harness.keys("f5");
    assert!(matches!(
        harness.app.overlays.last(),
        Some(Overlay::Confirm(_))
    ));
    harness.key(KeyChord::plain(KeyCode::Esc));
    harness.settle().await;
    assert!(
        harness.active_editor().results.is_empty(),
        "nothing executed after cancelling"
    );
    harness.keys("f5");
    harness.key(KeyChord::plain(KeyCode::Enter));
    harness.settle().await;
    assert!(matches!(
        harness.active_editor().results[0].kind,
        ResultKind::Affected(1)
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn key_value_browser_and_console() {
    let dir = tempfile::tempdir().unwrap();
    let kv = MockKeyValueDriver::with_keys([
        ("greeting", MockValue::String(b"hello".to_vec())),
        ("queue", MockValue::List(vec![b"a".to_vec(), b"b".to_vec()])),
    ]);
    let mut harness = Harness::new(dir.path(), vec![Arc::new(kv.clone())]);
    harness.app.startup(Some("Development"));
    let definition = DataSourceDefinition::new("Cache", DriverId::from("mock-kv"));
    let source = definition.id;
    harness.app.save_data_source(definition, None);
    harness.app.open_key_value_tab(source, false);
    harness.settle().await;
    let Some(Tab::KeyValue(tab)) = harness.app.active_tab() else {
        panic!("kv tab expected");
    };
    assert_eq!(tab.keys.len(), 2);
    assert!(matches!(tab.value, Some(ValueContent::Text { ref bytes, .. }) if bytes == b"hello"));
    harness.keys("j");
    harness.settle().await;
    let Some(Tab::KeyValue(tab)) = harness.app.active_tab() else {
        panic!("kv tab expected");
    };
    assert_eq!(tab.value.as_ref().map(ValueContent::len), Some(2));

    harness.keys(":");
    harness.type_text("PING");
    harness.key(KeyChord::plain(KeyCode::Enter));
    harness.settle().await;
    let Some(Tab::KeyValue(tab)) = harness.app.active_tab() else {
        panic!("kv tab expected");
    };
    assert!(tab.console_output.iter().any(|line| line.text == "PONG"));

    harness.key(KeyChord::plain(KeyCode::Esc));
    harness.keys("a");
    harness.type_text("hash user:1 name ada");
    harness.key(KeyChord::plain(KeyCode::Enter));
    harness.settle().await;
    assert!(kv.snapshot().contains_key(b"user:1".as_slice()));
}

#[tokio::test(flavor = "multi_thread")]
async fn palette_leader_keys_and_which_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = Harness::new(dir.path(), vec![Arc::new(MockKeyValueDriver::default())]);
    harness.app.startup(Some("Development"));
    harness.keys("ctrl+p");
    assert!(matches!(
        harness.app.overlays.last(),
        Some(Overlay::Picker(_))
    ));
    harness.type_text("split vert");
    harness.key(KeyChord::plain(KeyCode::Enter));
    assert_eq!(harness.app.workspace.pane_order().len(), 2);
    harness.app.focus = Focus::Sidebar;
    harness.key(KeyChord::plain(KeyCode::Char(' ')));
    assert_eq!(
        harness.app.pending_keys.len(),
        1,
        "leader pending for which-key"
    );
    harness.keys("x");
    assert!(harness.app.pending_keys.is_empty());
    assert_eq!(harness.app.workspace.pane_order().len(), 1);
    harness.key(KeyChord::new(KeyCode::F(1), Modifiers::NONE));
    assert!(matches!(
        harness.app.overlays.last(),
        Some(Overlay::Help { .. })
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_driver_reports_missing_package() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = Harness::new(dir.path(), vec![Arc::new(MockKeyValueDriver::default())]);
    harness.app.startup(Some("Development"));
    let definition = DataSourceDefinition::new("Exotic", DriverId::from("exampledb"));
    let source = definition.id;
    harness.app.save_data_source(definition, None);
    harness.app.connect(source);
    harness.settle().await;
    let messages: Vec<String> = harness
        .app
        .notifications
        .items()
        .iter()
        .map(|item| item.message.clone())
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("no installable package")),
        "{messages:?}"
    );
}
