/*
 * @author t0bx
 * @since 23.10.2026
 */

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use relix_app::Focus;
use relix_app::test_support::AppHarness;
use relix_config::{KeyChord, KeyCode};
use relix_core::{DataSourceDefinition, DriverId, Environment};
use relix_testing::mock_key_value::{MockKeyValueDriver, MockValue};
use relix_tui::colors::ColorDepth;
use relix_tui::ui::{UiState, draw};
use std::sync::Arc;

fn render(harness: &mut AppHarness, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    let mut ui = UiState::with_depth(&harness.app, ColorDepth::TrueColor);
    terminal
        .draw(|frame| draw(frame, &mut harness.app, &mut ui))
        .expect("draw");
    let buffer = terminal.backend().buffer().clone();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

fn harness(dir: &tempfile::TempDir) -> AppHarness {
    let kv =
        MockKeyValueDriver::with_keys([("session:1", MockValue::String(b"{\"user\":7}".to_vec()))]);
    let mut harness = AppHarness::new(dir.path(), vec![Arc::new(kv)]);
    harness.app.startup(Some("Development"));
    harness
}

#[tokio::test(flavor = "multi_thread")]
async fn main_layout_shows_header_explorer_panes_and_status() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = harness(&dir);
    harness.add_sqlite(
        "Local",
        &dir.path().join("local.db"),
        Environment::Development,
    );
    let screen = render(&mut harness, 120, 32);
    println!("{screen}");
    assert!(screen.contains("RELIX"));
    assert!(screen.contains("Project: Development"));
    assert!(screen.contains("DATA SOURCES"));
    assert!(screen.contains("Local"));
    assert!(screen.contains("EXPLORER"));
    assert!(screen.contains("Commands"));
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_results_grid_and_json_view_render() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = harness(&dir);
    let source = harness.add_sqlite(
        "Local",
        &dir.path().join("local.db"),
        Environment::Production,
    );
    harness
        .run_sql(source, "CREATE TABLE users (id INTEGER PRIMARY KEY, username TEXT, status TEXT);\nINSERT INTO users (username, status) VALUES ('admin', 'active'), ('user1', NULL);\nSELECT * FROM users;")
        .await;
    let screen = render(&mut harness, 140, 36);
    println!("{screen}");
    assert!(screen.contains("SELECT * FROM users;"));
    assert!(screen.contains("RESULTS"));
    assert!(screen.contains("username"));
    assert!(screen.contains("admin"));
    assert!(screen.contains("NULL"));
    assert!(screen.contains("PROD"));
    assert!(screen.contains("INSERT"));
    harness.keys("ctrl+down");
    harness.keys("v");
    let screen = render(&mut harness, 140, 36);
    println!("{screen}");
    assert!(screen.contains("\"username\": \"admin\""));
    assert!(screen.contains("RESULTS"));
}

#[tokio::test(flavor = "multi_thread")]
async fn table_tab_shows_filter_bar_sort_and_pending_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = harness(&dir);
    let source = harness.add_sqlite("Local", &dir.path().join("local.db"), Environment::None);
    harness
        .run_sql(source, "CREATE TABLE orders (id INTEGER PRIMARY KEY, total REAL); INSERT INTO orders (total) VALUES (10.5), (99);")
        .await;
    harness.app.focus = Focus::Sidebar;
    harness.app.explorer.selected = 0;
    harness.keys("l");
    harness.settle().await;
    harness.keys("j l");
    harness.settle().await;
    harness.keys("j l j enter");
    harness.settle().await;
    harness.keys("l s");
    harness.settle().await;
    harness.keys("e ctrl+u");
    harness.type_text("42");
    harness.key(KeyChord::plain(KeyCode::Enter));
    let screen = render(&mut harness, 120, 30);
    println!("{screen}");
    assert!(screen.contains("WHERE"));
    assert!(screen.contains("ORDER BY total ▲"));
    assert!(screen.contains("1 pending change"));
    assert!(screen.contains("⚷id"));
    assert!(screen.contains("GRID"));
}

#[tokio::test(flavor = "multi_thread")]
async fn key_value_tab_and_overlays_render() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = harness(&dir);
    let cache = harness.add_data_source(DataSourceDefinition::new(
        "Cache",
        DriverId::from("mock-kv"),
    ));
    harness.open_key_browser(cache).await;
    let screen = render(&mut harness, 120, 32);
    println!("{screen}");
    assert!(screen.contains("MATCH"));
    assert!(screen.contains("session:1"));
    assert!(screen.contains("STRING"));
    assert!(screen.contains("\"user\": 7"));
    assert!(screen.contains("CONSOLE"));

    harness.keys("ctrl+p");
    harness.type_text("split");
    let screen = render(&mut harness, 120, 32);
    assert!(screen.contains("Command palette"));
    assert!(screen.contains("Split pane vertically"));
    harness.key(KeyChord::plain(KeyCode::Esc));

    harness.keys("ctrl+k");
    let screen = render(&mut harness, 120, 32);
    println!("{screen}");
    assert!(
        screen.contains("Split pane vertically"),
        "which-key popup lists leader continuations"
    );
    harness.key(KeyChord::plain(KeyCode::Esc));

    harness.keys("f1");
    let screen = render(&mut harness, 120, 32);
    assert!(screen.contains("Keyboard shortcuts"));
}

#[tokio::test(flavor = "multi_thread")]
async fn data_source_form_and_small_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = harness(&dir);
    harness.keys("ctrl+k a");
    let screen = render(&mut harness, 120, 34);
    println!("{screen}");
    assert!(screen.contains("New data source"));
    assert!(screen.contains("Engine"));
    assert!(screen.contains("Ctrl+T test"));
    let tiny = render(&mut harness, 40, 10);
    assert!(tiny.contains("RELIX needs at least"));
}

#[tokio::test(flavor = "multi_thread")]
async fn server_configurations_manager_and_ssh_dialog_render() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = harness(&dir);
    harness.keys("ctrl+k n");
    let manager = render(&mut harness, 130, 30);
    println!("{manager}");
    assert!(manager.contains("Server configurations"));
    assert!(manager.contains("SSH"));
    assert!(manager.contains("press n to create an SSH configuration"));
    harness.keys("n");
    let dialog = render(&mut harness, 130, 34);
    println!("{dialog}");
    assert!(dialog.contains("New SSH configuration"));
    assert!(dialog.contains("SSH host"));
    assert!(dialog.contains("Jump host"));
    assert!(dialog.contains("Host keys"));
    assert!(dialog.contains("ask on first use"));
}

#[tokio::test(flavor = "multi_thread")]
async fn data_source_dialog_offers_network_modes() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = harness(&dir);
    harness.keys("ctrl+k a tab right");
    let dialog = render(&mut harness, 130, 40);
    println!("{dialog}");
    assert!(dialog.contains("New data source"));
    assert!(dialog.contains("Network"));
    assert!(dialog.contains("◂ direct ▸"));
    assert!(dialog.contains("Query timeout"));
    assert!(dialog.contains("Properties"));
    assert!(!dialog.contains("SSH config"));
}
