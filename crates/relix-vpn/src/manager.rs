/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::command::{CommandRunner, SystemCommand, SystemRunner};
use crate::elevation::ElevationMethod;
use crate::error::VpnError;
use crate::network::{NetworkInspector, SystemInspector, probe, split_address};
use crate::profile_file::{self, overlapping};
use crate::provider::{self, ProviderPlan};
use crate::tools::find_executable;
use ipnet::IpNet;
use relix_core::{ServerConfigId, VpnMode, VpnProfile};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

const READY_POLL: Duration = Duration::from_millis(300);
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const PROBE_DEFAULT_PORT: u16 = 443;

pub trait ToolLocator: Send + Sync {
    fn locate(&self, tool: &str) -> Option<PathBuf>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PathLocator;

impl ToolLocator for PathLocator {
    fn locate(&self, tool: &str) -> Option<PathBuf> {
        find_executable(tool)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    NotGranted,
    Granted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VpnStatus {
    Disconnected,
    Connected {
        owned: bool,
        interface: Option<String>,
        references: usize,
    },
    Failed(String),
}

impl VpnStatus {
    pub fn label(&self) -> String {
        match self {
            Self::Disconnected => "disconnected".into(),
            Self::Connected { owned: true, .. } => "connected (managed by Relix)".into(),
            Self::Connected { owned: false, .. } => "connected (system)".into(),
            Self::Failed(message) => format!("failed: {message}"),
        }
    }
}

struct Active {
    profile: VpnProfile,
    owned: bool,
    pinned: bool,
    references: usize,
    routes: Vec<IpNet>,
    plan: Option<ProviderPlan>,
    interface: Option<String>,
}

#[derive(Default)]
struct State {
    active: HashMap<ServerConfigId, Active>,
    failures: HashMap<ServerConfigId, String>,
}

struct Inner {
    runner: Arc<dyn CommandRunner>,
    inspector: Arc<dyn NetworkInspector>,
    tools: Arc<dyn ToolLocator>,
    elevation: Option<ElevationMethod>,
    runtime_dir: PathBuf,
    state: Mutex<State>,
}

#[derive(Clone)]
pub struct VpnManager {
    inner: Arc<Inner>,
}

impl VpnManager {
    pub fn system(runtime_dir: impl Into<PathBuf>) -> Self {
        Self::new(
            Arc::new(SystemRunner),
            Arc::new(SystemInspector),
            Arc::new(PathLocator),
            ElevationMethod::current(),
            runtime_dir,
        )
    }

    pub fn new(
        runner: Arc<dyn CommandRunner>,
        inspector: Arc<dyn NetworkInspector>,
        tools: Arc<dyn ToolLocator>,
        elevation: Option<ElevationMethod>,
        runtime_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                runner,
                inspector,
                tools,
                elevation,
                runtime_dir: runtime_dir.into(),
                state: Mutex::default(),
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub fn status(&self, profile: &VpnProfile) -> VpnStatus {
        let state = self.state();
        if let Some(active) = state.active.get(&profile.id) {
            return VpnStatus::Connected {
                owned: active.owned,
                interface: active.interface.clone(),
                references: active.references,
            };
        }
        if let Some(failure) = state.failures.get(&profile.id) {
            return VpnStatus::Failed(failure.clone());
        }
        drop(state);
        match provider::expected_interface(profile, self.inner.inspector.as_ref()) {
            Some(interface) if self.inner.inspector.interface(&interface).is_some() => {
                VpnStatus::Connected {
                    owned: false,
                    interface: Some(interface),
                    references: 0,
                }
            }
            _ => VpnStatus::Disconnected,
        }
    }

    /// The command Relix would run (before elevation) to bring a managed profile up, so
    /// the consent dialog can show exactly what will be executed.
    pub fn planned_command(&self, profile: &VpnProfile) -> Result<SystemCommand, VpnError> {
        Ok(self.resolve_plan(profile)?.up)
    }

    /// Makes sure the profile's network is available for one data source connection and
    /// returns a lease that keeps it so. System-managed VPNs are only verified, never
    /// changed. Managed profiles are started when they are down and set to connect
    /// automatically (or the user asked for it), which needs explicit approval because
    /// it runs a privileged command; Relix only owns, and later stops, what it started.
    pub async fn acquire(
        &self,
        profile: &VpnProfile,
        approval: Approval,
    ) -> Result<VpnLease, VpnError> {
        if self.join(profile.id) {
            return Ok(self.lease(profile.id));
        }
        match profile.mode {
            VpnMode::Existing => {
                let interface = self.verify_existing(profile).await?;
                self.register(profile, false, false, None, Vec::new(), interface);
            }
            VpnMode::Managed => {
                if let Some(interface) = self.detect_running(profile).await {
                    self.register(profile, false, false, None, Vec::new(), interface);
                } else if !profile.auto_connect && approval == Approval::NotGranted {
                    return Err(VpnError::NotConnected {
                        profile: profile.name.clone(),
                    });
                } else {
                    self.start(profile, approval, false).await?;
                }
            }
        }
        Ok(self.lease(profile.id))
    }

    pub async fn connect(&self, profile: &VpnProfile, approval: Approval) -> Result<(), VpnError> {
        if profile.mode == VpnMode::Existing {
            self.verify_existing(profile).await?;
            return Ok(());
        }
        if let Some(active) = self.state().active.get_mut(&profile.id) {
            active.pinned = true;
            return Ok(());
        }
        if let Some(interface) = self.detect_running(profile).await {
            self.register(profile, false, true, None, Vec::new(), interface);
            return Ok(());
        }
        self.start(profile, approval, true).await
    }

    pub async fn disconnect(&self, profile: &VpnProfile) -> Result<(), VpnError> {
        let active = {
            let mut state = self.state();
            let Some(active) = state.active.get(&profile.id) else {
                return Ok(());
            };
            if !active.owned {
                return Err(VpnError::NotOwned {
                    profile: profile.name.clone(),
                });
            }
            if active.references > 0 {
                return Err(VpnError::InUse {
                    profile: profile.name.clone(),
                    count: active.references,
                });
            }
            state.active.remove(&profile.id)
        };
        match active {
            Some(active) => self.stop(active).await,
            None => Ok(()),
        }
    }

    pub async fn diagnose(&self, profile: &VpnProfile, target: Option<(&str, u16)>) -> Vec<String> {
        let mut lines = vec![format!(
            "profile: {} ({})",
            profile.name,
            profile.mode.label()
        )];
        match provider::expected_interface(profile, self.inner.inspector.as_ref()) {
            Some(name) => match self.inner.inspector.interface(&name) {
                Some(interface) => lines.push(format!(
                    "interface {name}: up ({})",
                    interface
                        .addresses
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                None => lines.push(format!(
                    "interface {name}: not present — is the VPN connected?"
                )),
            },
            None => lines.push("interface: not configured (set one to verify the tunnel)".into()),
        }
        if let Some(address) = profile.probe_address.as_deref() {
            let (host, port) = split_address(address, PROBE_DEFAULT_PORT);
            lines.push(format!(
                "probe {address}: {}",
                probe(&host, port, PROBE_TIMEOUT).await.describe()
            ));
        }
        if let Some((host, port)) = target {
            lines.push(format!(
                "database {host}:{port}: {}",
                probe(host, port, PROBE_TIMEOUT).await.describe()
            ));
        }
        if let Some(path) = profile
            .config_path
            .as_deref()
            .filter(|_| profile.provider.is_manageable())
        {
            match profile_file::read(profile.provider, path) {
                Ok(file) if file.full_tunnel => {
                    lines.push("configuration: routes all traffic through the VPN".into())
                }
                Ok(file) => lines.push(format!(
                    "configuration: routes {}",
                    file.routes
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                Err(error) => lines.push(format!("configuration: {error}")),
            }
        }
        lines
    }

    async fn verify_existing(&self, profile: &VpnProfile) -> Result<Option<String>, VpnError> {
        let interface = provider::expected_interface(profile, self.inner.inspector.as_ref());
        if let Some(name) = &interface
            && self.inner.inspector.interface(name).is_none()
        {
            return Err(VpnError::InterfaceDown {
                profile: profile.name.clone(),
                interface: name.clone(),
            });
        }
        if let Some(address) = profile.probe_address.as_deref() {
            let (host, port) = split_address(address, PROBE_DEFAULT_PORT);
            let outcome = probe(&host, port, PROBE_TIMEOUT).await;
            if !outcome.is_reachable() {
                return Err(VpnError::Unreachable {
                    profile: profile.name.clone(),
                    target: address.to_owned(),
                    message: outcome.describe(),
                });
            }
        }
        Ok(interface)
    }

    async fn detect_running(&self, profile: &VpnProfile) -> Option<Option<String>> {
        let interface = provider::expected_interface(profile, self.inner.inspector.as_ref());
        match &interface {
            Some(name) => self
                .inner
                .inspector
                .interface(name)
                .map(|_| interface.clone()),
            None => None,
        }
    }

    async fn start(
        &self,
        profile: &VpnProfile,
        approval: Approval,
        pinned: bool,
    ) -> Result<(), VpnError> {
        let plan = self.resolve_plan(profile)?;
        if approval == Approval::NotGranted {
            return Err(VpnError::ApprovalRequired {
                profile: profile.name.clone(),
                command: plan.up.clone(),
            });
        }
        let routes = self.check_conflicts(profile)?;
        if let Some(pid_file) = &plan.pid_file
            && let Some(parent) = pid_file.parent()
        {
            let _ = std::fs::create_dir_all(parent);
        }
        let result = self.run_elevated(profile, &plan.up).await;
        if let Err(error) = result {
            self.state().failures.insert(profile.id, error.to_string());
            return Err(error);
        }
        let interface = self.wait_until_ready(profile, &plan).await?;
        self.register(profile, true, pinned, Some(plan), routes, interface);
        Ok(())
    }

    fn resolve_plan(&self, profile: &VpnProfile) -> Result<ProviderPlan, VpnError> {
        let mut plan = provider::plan(profile, &self.inner.runtime_dir)?;
        if let Some(path) = &profile.config_path {
            profile_file::read(profile.provider, path)?;
        }
        let located = self
            .inner
            .tools
            .locate(&plan.tool)
            .ok_or_else(|| VpnError::ToolMissing {
                tool: plan.tool.clone(),
                provider: profile.provider.label().into(),
            })?;
        plan.up.program = located.display().to_string();
        Ok(plan)
    }

    fn check_conflicts(&self, profile: &VpnProfile) -> Result<Vec<IpNet>, VpnError> {
        let routes = match &profile.config_path {
            Some(path) => profile_file::read(profile.provider, path)?.routes,
            None => Vec::new(),
        };
        let state = self.state();
        for active in state.active.values().filter(|active| active.owned) {
            let shared = overlapping(&routes, &active.routes);
            if !shared.is_empty() {
                return Err(VpnError::RouteConflict {
                    profile: profile.name.clone(),
                    other: active.profile.name.clone(),
                    routes: shared
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                });
            }
        }
        Ok(routes)
    }

    async fn run_elevated(
        &self,
        profile: &VpnProfile,
        command: &SystemCommand,
    ) -> Result<(), VpnError> {
        let elevation = self.inner.elevation.ok_or(VpnError::UnsupportedPlatform)?;
        let wrapped = elevation
            .wrap(command)
            .ok_or_else(|| VpnError::Configuration {
                profile: profile.name.clone(),
                message: "the command contains characters that cannot be passed safely".into(),
            })?;
        tracing::info!(profile = %profile.name, command = %command, "running privileged VPN command");
        let output =
            self.inner
                .runner
                .run(&wrapped)
                .await
                .map_err(|error| VpnError::StartFailed {
                    profile: profile.name.clone(),
                    message: error.to_string(),
                })?;
        if output.success {
            return Ok(());
        }
        if elevation.was_denied(&output) {
            return Err(VpnError::PrivilegeDenied {
                profile: profile.name.clone(),
            });
        }
        let message = Some(output.stderr.trim())
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| output.stdout.trim())
            .lines()
            .last()
            .unwrap_or("command failed")
            .to_owned();
        Err(VpnError::StartFailed {
            profile: profile.name.clone(),
            message,
        })
    }

    async fn wait_until_ready(
        &self,
        profile: &VpnProfile,
        plan: &ProviderPlan,
    ) -> Result<Option<String>, VpnError> {
        let deadline = Instant::now() + profile.connect_timeout();
        loop {
            let interface = provider::expected_interface(profile, self.inner.inspector.as_ref());
            let ready = match (&interface, profile.probe_address.as_deref()) {
                (Some(name), _) => self.inner.inspector.interface(name).is_some(),
                (None, Some(address)) => {
                    let (host, port) = split_address(address, PROBE_DEFAULT_PORT);
                    probe(&host, port, READY_POLL).await.is_reachable()
                }
                (None, None) => plan
                    .pid_file
                    .as_deref()
                    .is_none_or(|path| self.inner.inspector.read_file(path).is_some()),
            };
            if ready {
                return Ok(interface);
            }
            if Instant::now() >= deadline {
                return Err(VpnError::Timeout {
                    profile: profile.name.clone(),
                    seconds: profile.connect_timeout().as_secs(),
                });
            }
            tokio::time::sleep(READY_POLL).await;
        }
    }

    async fn stop(&self, active: Active) -> Result<(), VpnError> {
        let Some(plan) = &active.plan else {
            return Ok(());
        };
        let mut down =
            provider::down_command(&active.profile, plan, self.inner.inspector.as_ref())?;
        if let Some(located) = self.inner.tools.locate(&down.program) {
            down.program = located.display().to_string();
        }
        self.run_elevated(&active.profile, &down).await
    }

    fn join(&self, id: ServerConfigId) -> bool {
        let mut state = self.state();
        match state.active.get_mut(&id) {
            Some(active) => {
                active.references += 1;
                true
            }
            None => false,
        }
    }

    fn register(
        &self,
        profile: &VpnProfile,
        owned: bool,
        pinned: bool,
        plan: Option<ProviderPlan>,
        routes: Vec<IpNet>,
        interface: Option<String>,
    ) {
        let mut state = self.state();
        state.failures.remove(&profile.id);
        let references = usize::from(!pinned);
        state.active.insert(
            profile.id,
            Active {
                profile: profile.clone(),
                owned,
                pinned,
                references,
                routes,
                plan,
                interface,
            },
        );
    }

    fn lease(&self, id: ServerConfigId) -> VpnLease {
        VpnLease {
            manager: self.clone(),
            id,
        }
    }

    fn release(&self, id: ServerConfigId) {
        let finished = {
            let mut state = self.state();
            let Some(active) = state.active.get_mut(&id) else {
                return;
            };
            active.references = active.references.saturating_sub(1);
            let idle = active.references == 0 && !active.pinned;
            if !idle {
                return;
            }
            if active.owned && !active.profile.auto_disconnect {
                return;
            }
            state.active.remove(&id)
        };
        let Some(active) = finished.filter(|active| active.owned) else {
            return;
        };
        let manager = self.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if let Err(error) = manager.stop(active).await {
                    tracing::warn!(%error, "automatic VPN disconnect failed");
                }
            });
        }
    }

    /// Stops every tunnel Relix started whose profile asks for automatic disconnection;
    /// called when the application exits so no managed VPN outlives it unintentionally.
    /// Manually connected tunnels without auto-disconnect stay up.
    pub async fn shutdown(&self) {
        let owned: Vec<Active> = {
            let mut state = self.state();
            let ids: Vec<ServerConfigId> = state
                .active
                .iter()
                .filter(|(_, active)| active.owned && active.profile.auto_disconnect)
                .map(|(id, _)| *id)
                .collect();
            ids.into_iter()
                .filter_map(|id| state.active.remove(&id))
                .collect()
        };
        for active in owned {
            if let Err(error) = self.stop(active).await {
                tracing::warn!(%error, "VPN disconnect on exit failed");
            }
        }
    }

    pub fn runtime_dir(&self) -> &Path {
        &self.inner.runtime_dir
    }
}

pub struct VpnLease {
    manager: VpnManager,
    id: ServerConfigId,
}

impl VpnLease {
    pub fn profile_id(&self) -> ServerConfigId {
        self.id
    }
}

impl std::fmt::Debug for VpnLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VpnLease")
            .field("profile", &self.id)
            .finish()
    }
}

impl Drop for VpnLease {
    fn drop(&mut self) {
        self.manager.release(self.id);
    }
}
