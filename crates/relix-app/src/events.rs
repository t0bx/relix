/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::explorer::NewNode;
use crate::requests::RequestId;
use crate::services::InstallOffer;
use crate::workspace::TabId;
use relix_config::KeyChord;
use relix_connections::{ConnectionError, NetworkPlan, VpnError};
use relix_core::keyvalue::{CommandReply, KeyDescription, KeyName, KeyScanPage, ValuePage};
use relix_core::{
    ChangeReport, CollectionDetails, DataSourceId, DocumentPage, DocumentWriteOutcome, DriverId,
    RowBatch, ServerConfigId, TableDetails,
};
use relix_driver_api::{DatabaseConnection, DriverError, ServerInfo};
use relix_editor::{CatalogColumn, CompletionCatalog};
use relix_query::{ScriptOutcome, StatementReport};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    Click,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseInput {
    pub kind: MouseKind,
    pub column: u16,
    pub row: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyChord),
    Paste(String),
    Mouse(MouseInput),
    Tick,
}

pub enum AppEvent {
    Connected {
        source: DataSourceId,
        request: RequestId,
        result: Result<Arc<dyn DatabaseConnection>, ConnectionError>,
    },
    ConnectionTested {
        result: Result<(ServerInfo, String), ConnectionError>,
        plan: Option<NetworkPlan>,
    },
    SshTested {
        plan: NetworkPlan,
        result: Result<String, ConnectionError>,
    },
    VpnDiagnosed {
        profile: ServerConfigId,
        lines: Vec<String>,
    },
    VpnUpdated {
        profile: ServerConfigId,
        result: Result<String, VpnError>,
    },
    DriverOffer {
        source: DataSourceId,
        driver: DriverId,
        result: Result<Option<InstallOffer>, String>,
    },
    DriverInstalled {
        driver: DriverId,
        retry: Option<DataSourceId>,
        result: Result<String, String>,
    },
    DriverUpdates(Result<Vec<InstallOffer>, String>),
    ExplorerLoaded {
        key: String,
        result: Result<Vec<NewNode>, DriverError>,
    },
    Statement {
        tab: TabId,
        request: RequestId,
        report: StatementReport,
    },
    ScriptFinished {
        tab: TabId,
        request: RequestId,
        outcome: ScriptOutcome,
    },
    ResultPage {
        tab: TabId,
        index: usize,
        request: RequestId,
        result: Result<RowBatch, DriverError>,
    },
    TableDetails {
        tab: TabId,
        result: Result<TableDetails, DriverError>,
    },
    TablePage {
        tab: TabId,
        request: RequestId,
        result: Result<RowBatch, DriverError>,
    },
    RowCount {
        tab: TabId,
        result: Result<u64, DriverError>,
    },
    ChangesApplied {
        tab: TabId,
        result: Result<ChangeReport, DriverError>,
    },
    CatalogLoaded {
        source: DataSourceId,
        result: Result<CompletionCatalog, DriverError>,
    },
    CatalogColumns {
        source: DataSourceId,
        schema: Option<String>,
        table: String,
        result: Result<Vec<CatalogColumn>, DriverError>,
    },
    Definition {
        title: String,
        result: Result<Option<String>, DriverError>,
    },
    KeysScanned {
        tab: TabId,
        request: RequestId,
        result: Result<KeyScanPage, DriverError>,
    },
    KeyLoaded {
        tab: TabId,
        request: RequestId,
        key: KeyName,
        description: Option<KeyDescription>,
        result: Result<ValuePage, DriverError>,
    },
    KeyWritten {
        tab: TabId,
        result: Result<(), DriverError>,
        rescan: bool,
    },
    CommandReplied {
        tab: TabId,
        result: Result<CommandReply, DriverError>,
    },
    DocumentsLoaded {
        tab: TabId,
        request: RequestId,
        result: Result<DocumentPage, DriverError>,
    },
    DocumentWritten {
        tab: TabId,
        result: Result<DocumentWriteOutcome, DriverError>,
    },
    CollectionDescribed {
        tab: TabId,
        choose_index_to_drop: bool,
        result: Result<CollectionDetails, DriverError>,
    },
    ServerDetails {
        tab: TabId,
        result: Result<Vec<(String, String)>, DriverError>,
    },
}
