/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::ConnectionStatus;
use crate::command::Command;
use crate::overlay::{Overlay, PendingAction, PromptAction};
use crate::test_support::AppHarness as Harness;
use relix_config::{KeyChord, KeyCode};
use relix_core::{
    ConfigScope, DataSourceDefinition, DriverId, NetworkMode, SshAuthMethod, SshServerConfig,
};
use relix_testing::mock_key_value::MockKeyValueDriver;
use relix_testing::ssh_server::{PASSWORD, TestServer, ssh_config, start_echo};
use std::sync::Arc;

fn harness(dir: &std::path::Path) -> Harness {
    let mut harness = Harness::new(dir, vec![Arc::new(MockKeyValueDriver::default())]);
    harness.app.startup(Some("Homelab"));
    harness
}

fn top(harness: &Harness) -> Option<&Overlay> {
    harness.app.overlays.last()
}

fn ssh_names(harness: &Harness, scope: ConfigScope) -> Vec<String> {
    harness
        .app
        .servers
        .registry(scope)
        .ssh
        .iter()
        .map(|config| config.name.clone())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn server_configurations_are_created_renamed_duplicated_moved_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = harness(dir.path());
    harness.app.execute_command(Command::ServerConfigurations);
    assert!(matches!(top(&harness), Some(Overlay::ServerManager(_))));

    harness.keys("n");
    assert!(matches!(top(&harness), Some(Overlay::SshForm(_))));
    harness.type_text("Homelab SSH");
    harness.keys("tab tab");
    harness.type_text("192.168.178.10");
    harness.keys("tab tab");
    harness.type_text("admin");
    harness.keys("tab right tab");
    harness.type_text("s3cret-password");
    harness.key(KeyChord::plain(KeyCode::Enter));
    assert!(matches!(top(&harness), Some(Overlay::ServerManager(_))));
    assert_eq!(
        ssh_names(&harness, ConfigScope::Global),
        vec!["Homelab SSH"]
    );
    let saved = &harness.app.servers.registry(ConfigScope::Global).ssh[0];
    assert_eq!(saved.auth, SshAuthMethod::Password);
    assert_eq!(saved.username, "admin");
    let file = std::fs::read_to_string(harness.app.context().paths.servers_file()).unwrap();
    assert!(file.contains("Homelab SSH"));
    assert!(
        !file.contains("s3cret-password"),
        "secrets never reach configuration files"
    );

    harness.keys("r ctrl+u");
    harness.type_text("Homelab Gateway\n");
    assert_eq!(
        ssh_names(&harness, ConfigScope::Global),
        vec!["Homelab Gateway"]
    );

    harness.keys("y");
    assert_eq!(ssh_names(&harness, ConfigScope::Global).len(), 2);

    harness.keys("m");
    assert!(matches!(
        top(&harness),
        Some(Overlay::Confirm(confirm)) if matches!(confirm.action, PendingAction::TransferServerConfig { .. })
    ));
    harness.key(KeyChord::plain(KeyCode::Enter));
    assert_eq!(ssh_names(&harness, ConfigScope::Global).len(), 1);
    assert_eq!(ssh_names(&harness, ConfigScope::Project).len(), 1);

    harness.keys("d");
    harness.key(KeyChord::plain(KeyCode::Enter));
    let total = ssh_names(&harness, ConfigScope::Global).len()
        + ssh_names(&harness, ConfigScope::Project).len();
    assert_eq!(total, 1);

    let mut reloaded = Harness::new(dir.path(), vec![Arc::new(MockKeyValueDriver::default())]);
    reloaded.app.startup(Some("Homelab"));
    let reloaded_total = ssh_names(&reloaded, ConfigScope::Global).len()
        + ssh_names(&reloaded, ConfigScope::Project).len();
    assert_eq!(reloaded_total, 1, "configurations survive a restart");
}

#[tokio::test(flavor = "multi_thread")]
async fn data_sources_connect_through_a_trusted_ssh_tunnel() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(None).await;
    let (echo, _echo) = start_echo().await;
    let mut harness = harness(dir.path());
    let bastion: SshServerConfig = ssh_config("Homelab SSH", server.port, SshAuthMethod::Password);
    harness
        .app
        .save_ssh_config(ConfigScope::Global, bastion.clone(), None)
        .unwrap();
    let mut definition = DataSourceDefinition::new("Redis Homelab", DriverId::from("mock-kv"));
    definition.settings.host = Some("127.0.0.1".into());
    definition.settings.port = Some(echo);
    definition.network.mode = NetworkMode::SshTunnel;
    definition.network.ssh = Some(bastion.id);
    let source = harness.add_data_source(definition);

    harness.app.connect(source);
    harness.settle().await;
    assert!(matches!(
        top(&harness),
        Some(Overlay::Confirm(confirm)) if matches!(confirm.action, PendingAction::TrustHostKey { .. })
    ));
    harness.key(KeyChord::plain(KeyCode::Enter));
    harness.settle().await;
    assert!(matches!(
        top(&harness),
        Some(Overlay::Prompt(prompt)) if matches!(prompt.action, PromptAction::ServerSecret { .. })
    ));
    harness.type_text(PASSWORD);
    harness.key(KeyChord::plain(KeyCode::Enter));
    harness.settle().await;

    assert_eq!(
        harness.app.connection_status(source),
        ConnectionStatus::Connected
    );
    let indicator = harness.app.connection_indicator(source).unwrap();
    assert_eq!(indicator.label, "connected via SSH Homelab SSH");
    assert!(!indicator.degraded);
    assert_eq!(harness.app.context().network.ssh_sessions().len(), 1);

    harness.app.disconnect(source);
    harness.settle().await;
    assert!(harness.app.context().network.ssh_sessions().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn deleted_ssh_configurations_fail_with_a_clear_message() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = harness(dir.path());
    let mut definition = DataSourceDefinition::new("Orphan", DriverId::from("mock-kv"));
    definition.network.mode = NetworkMode::SshTunnel;
    definition.network.ssh = Some(relix_core::ServerConfigId::new());
    let source = harness.add_data_source(definition);
    harness.app.connect(source);
    harness.settle().await;
    let ConnectionStatus::Failed(message) = harness.app.connection_status(source) else {
        panic!("a missing configuration must fail");
    };
    assert!(message.contains("no longer exists"), "{message}");
}
