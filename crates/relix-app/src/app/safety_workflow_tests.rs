/*
 * @author t0bx
 * @since 27.10.2026
 */

use crate::editor_tab::ResultKind;
use crate::loaders;
use crate::notifications::Level;
use crate::overlay::Overlay;
use crate::tab::Tab;
use crate::test_support::AppHarness as Harness;
use relix_config::{KeyChord, KeyCode};
use relix_core::{DataSourceDefinition, DriverId, Environment};
use relix_driver_api::ExecuteOptions;
use relix_testing::mock_key_value::{MockKeyValueDriver, MockValue};
use std::sync::Arc;

fn harness(dir: &tempfile::TempDir) -> Harness {
    let mut harness = Harness::new(dir.path(), vec![Arc::new(MockKeyValueDriver::default())]);
    harness.app.startup(Some("Safety"));
    harness
}

fn latest_error(harness: &Harness) -> String {
    let notification = harness.app.notifications.latest().expect("a notification");
    assert_eq!(notification.level, Level::Error);
    notification.message.clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn production_sources_are_read_only_in_relix_and_in_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("orders.db");
    let mut harness = harness(&dir);
    let local = harness.add_sqlite("Local", &db, Environment::Development);
    harness
        .run_sql(
            local,
            "CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);",
        )
        .await;
    let production = harness.add_sqlite("Orders", &db, Environment::Production);

    harness.run_sql(production, "SELECT count(*) FROM t;").await;
    assert_eq!(harness.active_editor().results.len(), 1);

    harness
        .run_sql(production, "SELECT 1;\nDELETE FROM t WHERE id = 1;")
        .await;
    assert!(
        harness.active_editor().results.is_empty(),
        "a script containing a write is rejected as a whole"
    );
    let message = latest_error(&harness);
    assert!(message.contains("'Orders' · PROD"));
    assert!(message.contains("read-only (enforced by the database)"));
    assert!(message.contains("DELETE FROM t WHERE id = 1"));

    let connection = harness.app.connection(production).expect("connected");
    let sql = loaders::sql(connection.as_ref()).unwrap();
    assert!(
        sql.execute("DELETE FROM t", &ExecuteOptions::default())
            .await
            .is_err(),
        "the session itself refuses writes"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn staging_sources_confirm_writes_once_with_their_target() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = harness(&dir);
    let staging = harness.add_sqlite("Stage", &dir.path().join("stage.db"), Environment::Staging);

    harness
        .run_sql(staging, "CREATE TABLE t (id INTEGER PRIMARY KEY);")
        .await;
    let Some(Overlay::Confirm(confirm)) = harness.app.overlays.last() else {
        panic!("writes on staging need confirmation");
    };
    assert_eq!(confirm.lines[0], "Target: 'Stage' · staging");
    assert_eq!(confirm.lines[1], "Policy: confirm writes");
    assert!(confirm.danger);

    harness.key(KeyChord::plain(KeyCode::Enter));
    harness
        .settle_until(|harness| !harness.active_editor().results.is_empty())
        .await;
    assert!(
        !matches!(harness.app.overlays.last(), Some(Overlay::Confirm(_))),
        "connecting after the confirmation does not ask again"
    );
    assert!(matches!(
        harness.active_editor().results[0].kind,
        ResultKind::Message(_) | ResultKind::Affected(_)
    ));

    harness.run_sql(staging, "SELECT * FROM t;").await;
    assert!(harness.app.overlays.is_empty(), "reads are never confirmed");
}

#[tokio::test(flavor = "multi_thread")]
async fn read_only_key_value_sources_only_run_reading_commands() {
    let dir = tempfile::tempdir().unwrap();
    let kv = MockKeyValueDriver::with_keys([("greeting", MockValue::String(b"hello".to_vec()))]);
    let mut harness = Harness::new(dir.path(), vec![Arc::new(kv.clone())]);
    harness.app.startup(Some("Safety"));
    let mut definition = DataSourceDefinition::new("Cache", DriverId::from("mock-kv"));
    definition.environment = Environment::Production;
    let source = harness.add_data_source(definition);
    harness.open_key_browser(source).await;

    harness.keys(":");
    harness.type_text("SET greeting bye");
    harness.key(KeyChord::plain(KeyCode::Enter));
    harness.settle().await;
    assert!(latest_error(&harness).contains("read-only (checked by RELIX only)"));
    let Some(Tab::KeyValue(tab)) = harness.app.active_tab() else {
        panic!("kv tab expected");
    };
    assert_eq!(
        tab.console_input.text(),
        "SET greeting bye",
        "a blocked command stays editable"
    );
    assert_eq!(
        kv.snapshot().get(b"greeting".as_slice()),
        Some(&MockValue::String(b"hello".to_vec()))
    );

    harness.keys("ctrl+u");
    harness.type_text("GET greeting");
    harness.key(KeyChord::plain(KeyCode::Enter));
    harness.settle().await;
    let Some(Tab::KeyValue(tab)) = harness.app.active_tab() else {
        panic!("kv tab expected");
    };
    assert!(
        tab.console_output
            .iter()
            .any(|line| line.text.contains("hello"))
    );
}
