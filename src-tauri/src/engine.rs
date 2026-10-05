use crate::config;
use crate::logs::{self, LogStore};
use crate::ports;
use crate::types::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

const AZURITE_ACCOUNT_KEY: &str = "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";
const ACCOUNT_NAME: &str = "devstoreaccount1";
const SERVICES: [ServiceName; 3] = [ServiceName::Blob, ServiceName::Queue, ServiceName::Table];
/// Snapshots carry at most this many log lines per service / merged per
/// instance; `get_logs` returns the full buffer.
const SNAPSHOT_SERVICE_LOGS: usize = 500;
const SNAPSHOT_MERGED_LOGS: usize = 1_500;

pub type EventHandler = Arc<dyn Fn(EngineEvent) + Send + Sync + 'static>;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum EngineEvent {
    SnapshotUpdated(AppSnapshot),
    InstanceUpdated(InstanceSnapshot),
    ServiceUpdated(ServiceSnapshot),
    LogEntry(LogEntry),
    EngineUpdated(EngineSnapshot),
    McpUpdated(McpStatus),
}

type Key = (String, ServiceName);

fn key(instance_id: &str, service: &ServiceName) -> Key {
    (instance_id.to_string(), service.clone())
}

#[derive(Clone)]
pub struct AppEngine {
    inner: Arc<Mutex<EngineInner>>,
}

struct EngineInner {
    config: AppConfig,
    config_error: Option<String>,
    engine: EngineSnapshot,
    records: BTreeMap<Key, ServiceRecord>,
    processes: BTreeMap<Key, ManagedProcess>,
    /// Keys with a start in flight (reserved atomically with the
    /// `processes` check so concurrent starts cannot both spawn).
    starting: BTreeSet<Key>,
    logs: LogStore,
    mcp_status: McpStatus,
    event_handler: Option<EventHandler>,
}

struct ServiceRecord {
    state: ServiceState,
    started_at: Option<String>,
    started_instant: Option<Instant>,
    stopped_at: Option<String>,
    pid: Option<u32>,
    identity: Option<ProcessIdentity>,
    port_owner: Option<PortOwner>,
    exit_code: Option<i32>,
    error: Option<String>,
    owned_pids: HashSet<u32>,
}

/// Observable per-service state compared across a refresh.
type Fingerprint = (Key, ServiceState, Option<u32>, Option<u32>, Option<i32>, Option<String>);

struct ManagedProcess {
    child: Arc<Mutex<Child>>,
    root_pid: u32,
}

struct LaunchSpec {
    program: String,
    args: Vec<String>,
    display: String,
}

// ---------------------------------------------------------------------------
// Pure helpers (selectors, ports, connection strings, state)
// ---------------------------------------------------------------------------

fn describe_instances(config: &AppConfig) -> String {
    config
        .instances
        .iter()
        .map(|instance| format!("{} (\"{}\")", instance.id, instance.name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Resolve an instance selector (id, else case-insensitive name). `None` or
/// blank follows the default rule: the `default` instance, else the only
/// instance, else an error listing the available ids.
pub fn resolve_selector(config: &AppConfig, selector: Option<&str>) -> Result<String, String> {
    let selector = selector.map(str::trim).filter(|value| !value.is_empty());
    match selector {
        Some(wanted) => config
            .instances
            .iter()
            .find(|instance| instance.id == wanted)
            .or_else(|| config.instances.iter().find(|instance| instance.name.trim().eq_ignore_ascii_case(wanted)))
            .or_else(|| config.instances.iter().find(|instance| instance.id.eq_ignore_ascii_case(wanted)))
            .map(|instance| instance.id.clone())
            .ok_or_else(|| format!("no instance matches \"{wanted}\". Available: {}", describe_instances(config))),
        None => {
            if config.instances.iter().any(|instance| instance.id == DEFAULT_INSTANCE_ID) {
                Ok(DEFAULT_INSTANCE_ID.to_string())
            } else if config.instances.len() == 1 {
                Ok(config.instances[0].id.clone())
            } else {
                Err(format!("multiple instances; specify instance. Available: {}", describe_instances(config)))
            }
        }
    }
}

/// Ports no new instance may use: every configured instance port (running or
/// not) plus the MCP port range.
fn taken_ports(config: &AppConfig) -> HashSet<u16> {
    let mut taken: HashSet<u16> = config
        .instances
        .iter()
        .flat_map(|instance| instance.ports.values().copied())
        .collect();
    let (low, high) = config::mcp_port_range(&config.mcp);
    taken.extend(low..=high);
    taken
}

fn allocate_ports(config: &AppConfig, host: &str) -> Result<BTreeMap<ServiceName, u16>, String> {
    let trio = ports::find_free_port_trio(host, &taken_ports(config), config::ALLOC_START_PORT, config::ALLOC_END_PORT)
        .ok_or_else(|| {
            format!(
                "no free port trio between {} and {}; specify ports explicitly",
                config::ALLOC_START_PORT,
                config::ALLOC_END_PORT + 2
            )
        })?;
    Ok(SERVICES.iter().cloned().zip(trio).collect())
}

fn display_host(host: &str) -> String {
    match host.trim().trim_matches(['[', ']']) {
        "0.0.0.0" | "::" | "0:0:0:0:0:0:0:0" | "" => "127.0.0.1".to_string(),
        other => other.to_string(),
    }
}

fn build_connection_info(instance: &InstanceConfig) -> ConnectionInfo {
    let host = display_host(&instance.host);
    let prefix = format!("DefaultEndpointsProtocol=http;AccountName={ACCOUNT_NAME};AccountKey={AZURITE_ACCOUNT_KEY};");
    let mut endpoints = BTreeMap::new();
    let mut connection_strings = BTreeMap::new();
    let mut combined = prefix.clone();
    for service in SERVICES {
        let port = instance.ports.get(&service).copied().unwrap_or(0);
        let endpoint = format!("http://{host}:{port}/{ACCOUNT_NAME}");
        let label = match service {
            ServiceName::Blob => "Blob",
            ServiceName::Queue => "Queue",
            ServiceName::Table => "Table",
        };
        connection_strings.insert(service.clone(), format!("{prefix}{label}Endpoint={endpoint};"));
        combined.push_str(&format!("{label}Endpoint={endpoint};"));
        endpoints.insert(service, endpoint);
    }
    ConnectionInfo {
        instance_id: instance.id.clone(),
        account_name: ACCOUNT_NAME.to_string(),
        account_key: AZURITE_ACCOUNT_KEY.to_string(),
        endpoints,
        connection_strings,
        connection_string: combined,
    }
}

fn derive_instance_state(states: &[ServiceState]) -> InstanceState {
    if states.contains(&ServiceState::Starting) {
        InstanceState::Starting
    } else if states.iter().any(|state| matches!(state, ServiceState::Broken | ServiceState::PortInUse)) {
        InstanceState::Broken
    } else if !states.is_empty() && states.iter().all(|state| *state == ServiceState::Running) {
        InstanceState::Running
    } else if states.iter().all(|state| *state == ServiceState::Stopped) {
        InstanceState::Stopped
    } else {
        InstanceState::Partial
    }
}

fn azurite_args(instance: &InstanceConfig, service: &ServiceName) -> Result<Vec<String>, String> {
    let label = service_label(service);
    let port = instance.ports.get(service).copied().ok_or_else(|| format!("missing {label} port"))?;
    let mut args = vec![
        format!("--{label}Host"),
        instance.host.clone(),
        format!("--{label}Port"),
        port.to_string(),
        "--location".into(),
        instance.data_directory.clone(),
    ];
    if instance.loose {
        args.push("--loose".into());
    }
    if instance.skip_api_version_check {
        args.push("--skipApiVersionCheck".into());
    }
    Ok(args)
}

fn empty_to_none(value: Option<String>) -> Option<String> {
    value.map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

impl Default for AppEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AppEngine {
    pub fn new() -> Self {
        let loaded = config::load();
        logs::app_log(
            LogLevel::Info,
            &format!(
                "AzTray v{} starting; config {} ({} instance(s))",
                env!("CARGO_PKG_VERSION"),
                config::config_path().display(),
                loaded.config.instances.len()
            ),
        );
        if let Some(error) = &loaded.error {
            logs::app_log(LogLevel::Warn, &format!("config warning: {error}"));
        }
        let engine = inspect_engine(&loaded.config, config::validate(&loaded.config).err());
        let mcp_status = McpStatus::initial(&loaded.config.mcp);
        Self {
            inner: Arc::new(Mutex::new(EngineInner {
                config: loaded.config,
                config_error: loaded.error,
                engine,
                records: BTreeMap::new(),
                processes: BTreeMap::new(),
                starting: BTreeSet::new(),
                logs: LogStore::default(),
                mcp_status,
                event_handler: None,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, EngineInner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn set_event_handler(&self, handler: Option<EventHandler>) {
        self.lock().event_handler = handler;
    }

    // ---- snapshots ----------------------------------------------------------

    pub fn snapshot(&self) -> AppSnapshot {
        self.refresh_services();
        snapshot_locked(&self.lock())
    }

    pub fn get_snapshot(&self) -> AppSnapshot {
        self.snapshot()
    }

    pub fn list_instances(&self) -> Vec<InstanceSnapshot> {
        self.snapshot().instances
    }

    pub fn get_instance(&self, selector: Option<&str>) -> Result<InstanceSnapshot, String> {
        let id = self.resolve_instance_id(selector)?;
        self.snapshot()
            .instances
            .into_iter()
            .find(|instance| instance.config.id == id)
            .ok_or_else(|| format!("instance \"{id}\" no longer exists"))
    }

    pub fn resolve_instance_id(&self, selector: Option<&str>) -> Result<String, String> {
        resolve_selector(&self.lock().config, selector)
    }

    pub fn check_engine(&self) -> EngineSnapshot {
        let config = self.lock().config.clone();
        let snapshot = inspect_engine(&config, config::validate(&config).err());
        self.lock().engine = snapshot.clone();
        self.emit(EngineEvent::EngineUpdated(snapshot.clone()));
        snapshot
    }

    // ---- settings / instance CRUD ---------------------------------------------

    pub fn set_settings(&self, settings: GlobalSettings) -> Result<AppSnapshot, String> {
        {
            let mut inner = self.lock();
            let executable_path = empty_to_none(settings.executable_path);
            let node_path = empty_to_none(settings.node_path);
            let engine_paths_changed =
                executable_path != inner.config.executable_path || node_path != inner.config.node_path;
            if engine_paths_changed && !inner.processes.is_empty() {
                return Err("stop Azurite services before changing the Azurite or Node paths".into());
            }
            let mut next = inner.config.clone();
            next.executable_path = executable_path;
            next.node_path = node_path;
            next.mcp = settings.mcp;
            config::validate(&next)?;
            config::persist(&next)?;
            inner.config = next;
            inner.config_error = None;
        }
        logs::app_log(LogLevel::Info, "settings saved");
        self.check_engine();
        Ok(self.finish(None))
    }

    pub fn suggest_instance(&self, name: Option<&str>) -> InstanceDraft {
        let config = self.lock().config.clone();
        let taken_names: Vec<String> = config.instances.iter().map(|i| i.name.trim().to_lowercase()).collect();
        let name = match name.map(str::trim).filter(|value| !value.is_empty()) {
            Some(name) => name.to_string(),
            None => {
                let mut n = config.instances.len() + 1;
                loop {
                    let candidate = format!("Instance {n}");
                    if !taken_names.contains(&candidate.to_lowercase()) {
                        break candidate;
                    }
                    n += 1;
                }
            }
        };
        let ids: Vec<String> = config.instances.iter().map(|i| i.id.clone()).collect();
        let id = config::unique_id(&config::slugify(&name), &ids);
        let host = config::DEFAULT_HOST.to_string();
        let ports = allocate_ports(&config, &host).unwrap_or_else(|_| {
            let next = taken_ports(&config).into_iter().max().unwrap_or(10_000).saturating_add(1).min(65_533);
            SERVICES.iter().cloned().zip([next, next + 1, next + 2]).collect()
        });
        InstanceDraft {
            name,
            data_directory: config::instance_data_directory(&id).to_string_lossy().into_owned(),
            id,
            host,
            ports,
        }
    }

    pub fn create_instance(&self, req: CreateInstanceRequest) -> Result<InstanceSnapshot, String> {
        let explicit_ports = req.ports.is_some();
        let created = {
            let mut inner = self.lock();
            let name = req.name.trim().to_string();
            let existing_ids: Vec<String> = inner.config.instances.iter().map(|i| i.id.clone()).collect();
            let id = match req.id.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
                Some(id) => {
                    if !config::is_valid_id(id) {
                        return Err(format!("instance id \"{id}\" is invalid; use lowercase letters, digits, and hyphens (max {})", config::MAX_ID_CHARS));
                    }
                    id.to_string()
                }
                None => config::unique_id(&config::slugify(&name), &existing_ids),
            };
            let host = empty_to_none(req.host.clone()).unwrap_or_else(|| config::DEFAULT_HOST.to_string());
            let ports = match req.ports.clone() {
                Some(ports) => ports,
                None => allocate_ports(&inner.config, &host)?,
            };
            let data_directory = empty_to_none(req.data_directory.clone())
                .unwrap_or_else(|| config::instance_data_directory(&id).to_string_lossy().into_owned());
            let instance = InstanceConfig {
                id,
                name,
                host,
                ports,
                data_directory,
                loose: req.loose,
                skip_api_version_check: req.skip_api_version_check,
            };
            config::validate_instance(&instance)?;
            config::check_instance_conflicts(&inner.config, &instance, None)?;
            let mut next = inner.config.clone();
            next.instances.push(instance.clone());
            config::validate(&next)?;
            config::persist(&next)?;
            inner.config = next;
            inner.config_error = None;
            instance
        };
        logs::app_log(
            LogLevel::Info,
            &format!("created instance {} (\"{}\") ports {:?} data {}", created.id, created.name, created.ports.values().collect::<Vec<_>>(), created.data_directory),
        );
        if explicit_ports {
            self.warn_busy_ports(&created);
        }
        if req.start {
            let engine = self.check_engine();
            if let Err(error) = self.start_instance_inner(&created.id, &engine) {
                logs::app_log(LogLevel::Warn, &format!("instance {} created but did not fully start: {error}", created.id));
            }
        }
        // One refresh: `finish` returns the snapshot we pick the instance from.
        let snapshot = self.finish(Some(&created.id));
        snapshot
            .instances
            .into_iter()
            .find(|instance| instance.config.id == created.id)
            .ok_or_else(|| format!("instance \"{}\" no longer exists", created.id))
    }

    pub fn update_instance(&self, req: UpdateInstanceRequest) -> Result<InstanceSnapshot, String> {
        let (id, updated) = {
            let mut inner = self.lock();
            let id = resolve_selector(&inner.config, Some(&req.instance_id))?;
            let current = inner
                .config
                .instances
                .iter()
                .find(|instance| instance.id == id)
                .cloned()
                .ok_or_else(|| format!("instance \"{id}\" no longer exists"))?;
            let mut next_instance = current.clone();
            if let Some(name) = &req.name {
                next_instance.name = name.trim().to_string();
            }
            if let Some(host) = empty_to_none(req.host.clone()) {
                next_instance.host = host;
            }
            if let Some(ports) = &req.ports {
                next_instance.ports = ports.clone();
            }
            if let Some(directory) = empty_to_none(req.data_directory.clone()) {
                next_instance.data_directory = directory;
            }
            if let Some(loose) = req.loose {
                next_instance.loose = loose;
            }
            if let Some(skip) = req.skip_api_version_check {
                next_instance.skip_api_version_check = skip;
            }
            let structural = InstanceConfig { name: current.name.clone(), ..next_instance.clone() } != current;
            if structural && inner.processes.keys().any(|(instance_id, _)| *instance_id == id) {
                return Err(format!(
                    "stop instance \"{}\" before changing its host, ports, data directory, or flags",
                    current.name
                ));
            }
            config::validate_instance(&next_instance)?;
            config::check_instance_conflicts(&inner.config, &next_instance, Some(&id))?;
            let mut next = inner.config.clone();
            if let Some(slot) = next.instances.iter_mut().find(|instance| instance.id == id) {
                *slot = next_instance.clone();
            }
            config::validate(&next)?;
            config::persist(&next)?;
            inner.config = next;
            inner.config_error = None;
            if structural {
                // Stale per-service state (e.g. portInUse on the old port) no longer applies.
                inner.records.retain(|(instance_id, _), _| *instance_id != id);
            }
            (id, next_instance)
        };
        logs::app_log(LogLevel::Info, &format!("updated instance {id} (\"{}\")", updated.name));
        if req.ports.is_some() {
            self.warn_busy_ports(&updated);
        }
        let snapshot = self.finish(Some(&id));
        snapshot
            .instances
            .into_iter()
            .find(|instance| instance.config.id == id)
            .ok_or_else(|| format!("instance \"{id}\" no longer exists"))
    }

    pub fn delete_instance(&self, selector: &str) -> Result<AppSnapshot, String> {
        let id = {
            let mut inner = self.lock();
            let id = resolve_selector(&inner.config, Some(selector))?;
            if inner.processes.keys().any(|(instance_id, _)| *instance_id == id) {
                return Err(format!("stop instance \"{id}\" before deleting it"));
            }
            if inner.config.instances.len() <= 1 {
                return Err("the last remaining instance cannot be deleted".into());
            }
            let mut next = inner.config.clone();
            next.instances.retain(|instance| instance.id != id);
            config::validate(&next)?;
            config::persist(&next)?;
            inner.config = next;
            inner.config_error = None;
            inner.records.retain(|(instance_id, _), _| *instance_id != id);
            inner.logs.remove_instance(&id);
            id
        };
        logs::app_log(LogLevel::Info, &format!("deleted instance {id} (data on disk untouched)"));
        Ok(self.finish(None))
    }

    fn warn_busy_ports(&self, instance: &InstanceConfig) {
        for (service, port) in &instance.ports {
            if !ports::is_port_free(&instance.host, *port) {
                logs::app_log(
                    LogLevel::Warn,
                    &format!("instance {}: {} port {port} is currently in use by another process", instance.id, service_label(service)),
                );
            }
        }
    }

    // ---- lifecycle ---------------------------------------------------------------

    pub fn start_instance(&self, selector: Option<&str>) -> Result<AppSnapshot, String> {
        let id = self.resolve_instance_id(selector)?;
        let engine = self.check_engine();
        let result = self.start_instance_inner(&id, &engine);
        let snapshot = self.finish(Some(&id));
        result.map(|_| snapshot)
    }

    pub fn stop_instance(&self, selector: Option<&str>) -> Result<AppSnapshot, String> {
        let id = self.resolve_instance_id(selector)?;
        let result = self.stop_instance_inner(&id);
        let snapshot = self.finish(Some(&id));
        result.map(|_| snapshot)
    }

    pub fn restart_instance(&self, selector: Option<&str>) -> Result<AppSnapshot, String> {
        let id = self.resolve_instance_id(selector)?;
        let result = self.stop_instance_inner(&id).and_then(|_| {
            let engine = self.check_engine();
            self.start_instance_inner(&id, &engine)
        });
        let snapshot = self.finish(Some(&id));
        result.map(|_| snapshot)
    }

    pub fn start_service(&self, selector: Option<&str>, service: ServiceName) -> Result<AppSnapshot, String> {
        let id = self.resolve_instance_id(selector)?;
        let engine = self.check_engine();
        let result = self.start_one(&id, &service, &engine);
        let snapshot = self.finish(Some(&id));
        result.map(|_| snapshot)
    }

    pub fn stop_service(&self, selector: Option<&str>, service: ServiceName) -> Result<AppSnapshot, String> {
        let id = self.resolve_instance_id(selector)?;
        let result = self.stop_one(&id, &service);
        let snapshot = self.finish(Some(&id));
        result.map(|_| snapshot)
    }

    pub fn restart_service(&self, selector: Option<&str>, service: ServiceName) -> Result<AppSnapshot, String> {
        let id = self.resolve_instance_id(selector)?;
        let result = self.stop_one(&id, &service).and_then(|_| {
            let engine = self.check_engine();
            self.start_one(&id, &service, &engine)
        });
        let snapshot = self.finish(Some(&id));
        result.map(|_| snapshot)
    }

    pub fn start_all(&self) -> Result<AppSnapshot, String> {
        let ids = self.instance_ids();
        let engine = self.check_engine();
        let mut first_error = None;
        for id in &ids {
            if let Err(error) = self.start_instance_inner(id, &engine) {
                first_error.get_or_insert(error);
            }
        }
        let snapshot = self.finish(None);
        first_error.map_or(Ok(snapshot), Err)
    }

    pub fn stop_all(&self) -> Result<AppSnapshot, String> {
        let mut first_error = None;
        for id in &self.instance_ids() {
            if let Err(error) = self.stop_instance_inner(id) {
                first_error.get_or_insert(error);
            }
        }
        let snapshot = self.finish(None);
        first_error.map_or(Ok(snapshot), Err)
    }

    pub fn restart_all(&self) -> Result<AppSnapshot, String> {
        self.stop_all()?;
        self.start_all()
    }

    fn instance_ids(&self) -> Vec<String> {
        self.lock().config.instances.iter().map(|instance| instance.id.clone()).collect()
    }

    /// Attempt all three services; return the first error afterwards.
    fn start_instance_inner(&self, id: &str, engine: &EngineSnapshot) -> Result<(), String> {
        let mut first_error = None;
        for service in SERVICES {
            if let Err(error) = self.start_one(id, &service, engine) {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    fn stop_instance_inner(&self, id: &str) -> Result<(), String> {
        let mut first_error = None;
        for service in SERVICES {
            if let Err(error) = self.stop_one(id, &service) {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Refresh, emit the full snapshot (and the instance, when given), return it.
    fn finish(&self, instance_id: Option<&str>) -> AppSnapshot {
        let snapshot = self.snapshot();
        self.emit(EngineEvent::SnapshotUpdated(snapshot.clone()));
        if let Some(id) = instance_id {
            if let Some(instance) = snapshot.instances.iter().find(|instance| instance.config.id == id) {
                self.emit(EngineEvent::InstanceUpdated(instance.clone()));
            }
        }
        snapshot
    }

    fn start_one(&self, id: &str, service: &ServiceName, engine: &EngineSnapshot) -> Result<(), String> {
        if engine.state != EngineState::Ready {
            let error = engine.message.clone().unwrap_or_else(|| "Azurite is not ready".into());
            self.record_start_error(id, service, error.clone());
            return Err(error);
        }
        let k = key(id, service);
        let looked_up = {
            let inner = self.lock();
            inner
                .config
                .instances
                .iter()
                .find(|instance| instance.id == id)
                .map(|instance| (inner.config.clone(), instance.clone()))
        };
        let Some((config, instance)) = looked_up else {
            return Err(format!("instance \"{id}\" no longer exists"));
        };
        let Some(port) = instance.ports.get(service).copied() else {
            let error = "service port is missing".to_string();
            self.record_start_error(id, service, error.clone());
            return Err(error);
        };
        // Atomically claim the key; a concurrent start (or a running process)
        // means there is nothing to do.
        if !self.try_reserve_start(&k) {
            return Ok(());
        }
        let result = self.launch_one(id, service, &config, &instance, port);
        self.lock().starting.remove(&k);
        result
    }

    /// Claim `k` for a start. False when a process is already managed or
    /// another start holds the reservation. The check and the claim happen
    /// under one lock acquisition.
    fn try_reserve_start(&self, k: &Key) -> bool {
        let mut inner = self.lock();
        if inner.processes.contains_key(k) {
            return false;
        }
        inner.starting.insert(k.clone())
    }

    fn launch_one(
        &self,
        id: &str,
        service: &ServiceName,
        config: &AppConfig,
        instance: &InstanceConfig,
        port: u16,
    ) -> Result<(), String> {
        let k = key(id, service);
        let probe = ports::probe(&instance.host, port, &HashSet::new());
        if let Some(owner) = probe.owner {
            let error = format!("{} port {} is used by PID {}", service_label(service), port, owner.process.pid);
            self.set_port_in_use(id, service, owner, error.clone());
            self.append_system_error(id, service, error.clone());
            return Err(error);
        }
        if let Some(error) = probe.error {
            self.set_service_error(id, service, ServiceState::Broken, error.clone());
            self.append_system_error(id, service, error.clone());
            return Err(error);
        }
        if let Err(error) = std::fs::create_dir_all(&instance.data_directory) {
            let error = format!("could not create Azurite data directory: {error}");
            self.record_start_error(id, service, error.clone());
            return Err(error);
        }
        let spec = match resolve_launch_spec(config, instance, service) {
            Ok(spec) => spec,
            Err(error) => {
                self.record_start_error(id, service, error.clone());
                return Err(error);
            }
        };
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide_console(&mut command);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let error = format!("could not start {}: {error}", spec.display);
                self.record_start_error(id, service, error.clone());
                return Err(error);
            }
        };
        let root_pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));
        let started_at = logs::now_timestamp();
        {
            let mut inner = self.lock();
            let record = inner.records.entry(k.clone()).or_default();
            record.state = ServiceState::Starting;
            record.started_at = Some(started_at);
            record.started_instant = Some(Instant::now());
            record.stopped_at = None;
            record.pid = Some(root_pid);
            record.identity = None;
            record.port_owner = None;
            record.exit_code = None;
            record.error = None;
            record.owned_pids.clear();
            record.owned_pids.insert(root_pid);
            inner.processes.insert(k, ManagedProcess { child: child.clone(), root_pid });
        }
        self.append_system_log(id, service, format!("Starting {} (port {port})", spec.display));
        self.emit_service_updated(id, service);
        if let Some(stdout) = stdout {
            self.spawn_log_reader(id.to_string(), service.clone(), LogStream::Stdout, stdout);
        }
        if let Some(stderr) = stderr {
            self.spawn_log_reader(id.to_string(), service.clone(), LogStream::Stderr, stderr);
        }
        let watcher = self.clone();
        let (watch_id, watch_service) = (id.to_string(), service.clone());
        thread::spawn(move || watcher.watch_start(&watch_id, &watch_service));
        Ok(())
    }

    fn stop_one(&self, id: &str, service: &ServiceName) -> Result<(), String> {
        let k = key(id, service);
        let managed = self.lock().processes.remove(&k);
        let Some(managed) = managed else {
            self.refresh_services();
            return Ok(());
        };
        if let Err(error) = ports::terminate(managed.root_pid, true) {
            // taskkill fails for a process that already exited; that is a stop,
            // not an error. Only a still-live process keeps the service running.
            let exited = managed.child.lock().map(|mut child| child.try_wait().ok().flatten().is_some()).unwrap_or(false)
                || !ports::process_alive(managed.root_pid);
            if !exited {
                self.lock().processes.insert(k, managed);
                return Err(error);
            }
        }
        wait_for_exit(&managed.child, Duration::from_secs(2));
        self.mark_stopped(id, service, None, None);
        self.append_system_log(id, service, "Stopped by user".into());
        Ok(())
    }

    // ---- ports --------------------------------------------------------------------

    pub fn identify_port_owner(&self, selector: Option<&str>, service: ServiceName) -> Option<PortOwner> {
        let id = self.resolve_instance_id(selector).ok()?;
        self.refresh_services();
        let (host, port, owned) = {
            let inner = self.lock();
            let instance = inner.config.instances.iter().find(|instance| instance.id == id)?;
            let port = *instance.ports.get(&service)?;
            let owned = inner.records.get(&key(&id, &service)).map(|record| record.owned_pids.clone()).unwrap_or_default();
            (instance.host.clone(), port, owned)
        };
        ports::probe(&host, port, &owned).owner
    }

    pub fn free_port(&self, expected: PortOwnerExpectation) -> Result<FreePortResult, String> {
        self.free_port_confirmed(expected, true)
    }

    pub fn free_port_confirmed(&self, expected: PortOwnerExpectation, confirmed: bool) -> Result<FreePortResult, String> {
        if !confirmed {
            return Err("free port requires explicit confirmation".into());
        }
        let id = self.resolve_instance_id(expected.instance_id.as_deref())?;
        let service = expected.service_name.clone();
        let current = self.identify_port_owner(Some(&id), service.clone());
        let Some(owner) = current else {
            return Ok(FreePortResult { snapshot: self.snapshot(), released: false, surviving_owner: None, message: "The port is already free.".into() });
        };
        if !ports::identity_matches(&owner, expected.pid, expected.started_at.as_deref()) {
            return Err("port owner changed; refusing to terminate a reused PID".into());
        }
        if owner.owned_by_app {
            self.stop_one(&id, &service)?;
        } else {
            ports::terminate(owner.process.pid, true)?;
            wait_for_pid_exit(owner.process.pid, Duration::from_secs(2));
            logs::app_log(LogLevel::Warn, &format!("terminated external PID {} holding {} port of instance {id}", owner.process.pid, service_label(&service)));
        }
        let surviving_owner = self.identify_port_owner(Some(&id), service);
        let released = surviving_owner.is_none();
        let message = if released { "Port released." } else { "The process survived or respawned." };
        let snapshot = self.finish(Some(&id));
        Ok(FreePortResult { snapshot, released, surviving_owner, message: message.into() })
    }

    // ---- logs ----------------------------------------------------------------------

    pub fn get_logs(&self, query: LogsQuery) -> Vec<LogEntry> {
        let inner = self.lock();
        match resolve_selector(&inner.config, query.instance_id.as_deref()) {
            Ok(id) => inner.logs.query(&id, &query),
            Err(_) => Vec::new(),
        }
    }

    pub fn save_logs(&self, args: SaveLogsArgs) -> Result<SaveLogsResult, String> {
        let inner = self.lock();
        let id = resolve_selector(&inner.config, args.instance_id.as_deref())?;
        inner.logs.save(&id, args.service_name.as_ref(), args.path.as_deref())
    }

    pub fn clear_logs(&self, selector: Option<&str>, service: Option<ServiceName>) -> AppSnapshot {
        let resolved = self.resolve_instance_id(selector).ok();
        if let Some(id) = &resolved {
            self.lock().logs.clear(id, service.as_ref());
        }
        self.finish(resolved.as_deref())
    }

    pub fn app_log_tail(&self, limit: usize) -> Vec<String> {
        logs::app_log_tail(limit)
    }

    // ---- connection provisioning ---------------------------------------------------

    pub fn connection_info(&self, selector: Option<&str>) -> Result<ConnectionInfo, String> {
        let inner = self.lock();
        let id = resolve_selector(&inner.config, selector)?;
        inner
            .config
            .instances
            .iter()
            .find(|instance| instance.id == id)
            .map(build_connection_info)
            .ok_or_else(|| format!("instance \"{id}\" no longer exists"))
    }

    pub fn connection_string(&self, selector: Option<&str>, service: Option<ServiceName>) -> Result<String, String> {
        let info = self.connection_info(selector)?;
        match service {
            Some(service) => info
                .connection_strings
                .get(&service)
                .cloned()
                .ok_or_else(|| format!("no connection string for {}", service_label(&service))),
            None => Ok(info.connection_string),
        }
    }

    // ---- MCP status plumbing ---------------------------------------------------------

    pub fn mcp_config(&self) -> McpConfig {
        self.lock().config.mcp.clone()
    }

    pub fn mcp_status(&self) -> McpStatus {
        self.lock().mcp_status.clone()
    }

    pub fn set_mcp_status(&self, status: McpStatus) {
        let previous = std::mem::replace(&mut self.lock().mcp_status, status.clone());
        if previous.running != status.running
            || previous.port != status.port
            || previous.error != status.error
            || previous.enabled != status.enabled
        {
            let level = if status.error.is_some() && !status.running { LogLevel::Warn } else { LogLevel::Info };
            let summary = match (&status.error, status.running) {
                (Some(error), false) => format!("failed: {error}"),
                (_, true) => format!("running at {}{}", status.url, if status.fallback_used { " (fallback port)" } else { "" }),
                (None, false) if !status.enabled => "disabled".to_string(),
                (None, false) => "not running".to_string(),
            };
            logs::app_log(level, &format!("MCP status: {summary} (attempt {})", status.attempts));
        }
        self.emit(EngineEvent::McpUpdated(status));
        // No service refresh here: MCP status changes must stay cheap.
        let snapshot = snapshot_locked(&self.lock());
        self.emit(EngineEvent::SnapshotUpdated(snapshot));
    }

    // ---- shutdown -------------------------------------------------------------------

    pub fn quit(&self, mode: QuitMode) -> Result<QuitResult, String> {
        if mode == QuitMode::Cancel {
            return Ok(QuitResult { mode, stopped_services: Vec::new() });
        }
        let mut stopped_services = Vec::new();
        if mode == QuitMode::StopAndQuit {
            let owned: Vec<Key> = self.lock().processes.keys().cloned().collect();
            for (instance_id, service) in owned {
                if self.stop_one(&instance_id, &service).is_ok() {
                    stopped_services.push(ServiceRef { instance_id, service_name: service });
                }
            }
        }
        logs::app_log(LogLevel::Info, &format!("quit requested ({mode:?}); stopped {} service(s)", stopped_services.len()));
        Ok(QuitResult { mode, stopped_services })
    }

    // ---- background work ------------------------------------------------------------

    fn watch_start(&self, id: &str, service: &ServiceName) {
        let k = key(id, service);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if Instant::now() >= deadline {
                let managed = self.lock().processes.remove(&k);
                if let Some(managed) = managed {
                    let _ = ports::terminate(managed.root_pid, true);
                }
                let error = "Azurite did not begin listening within 10 seconds".to_string();
                self.set_service_error(id, service, ServiceState::Broken, error.clone());
                self.append_system_error(id, service, format!("Startup timed out after 10 seconds: {error}"));
                self.emit_snapshot_updated();
                return;
            }
            let looked_up = {
                let inner = self.lock();
                let Some(managed) = inner.processes.get(&k) else { return };
                let Some(instance) = inner.config.instances.iter().find(|instance| instance.id == id) else { return };
                (
                    instance.host.clone(),
                    instance.ports.get(service).copied().unwrap_or(0),
                    managed.child.clone(),
                    managed.root_pid,
                )
            };
            let (host, port, managed, root_pid) = looked_up;
            if let Ok(mut child) = managed.lock() {
                if let Ok(Some(status)) = child.try_wait() {
                    self.lock().processes.remove(&k);
                    let error = format!("Azurite exited during startup ({status})");
                    self.set_service_error(id, service, ServiceState::Broken, error.clone());
                    self.append_system_error(id, service, error);
                    drop(child);
                    self.emit_snapshot_updated();
                    return;
                }
            }
            let owned = self.lock().records.get(&k).map(|record| record.owned_pids.clone()).unwrap_or_default();
            let probe = ports::probe(&host, port, &owned);
            if let Some(error) = probe.error {
                let error = format!("startup probe failed on {host}:{port}: {error}");
                let managed = self.lock().processes.remove(&k);
                if let Some(managed) = managed {
                    let _ = ports::terminate(managed.root_pid, true);
                }
                self.record_start_error(id, service, error);
                return;
            }
            if let Some(owner) = probe.owner {
                if owner.process.pid != 0 {
                    if !ports::is_descendant_or_self(owner.process.pid, root_pid) {
                        let managed = self.lock().processes.remove(&k);
                        if let Some(managed) = managed {
                            let _ = ports::terminate(managed.root_pid, true);
                        }
                        let error = "another process claimed the service port during startup".to_string();
                        self.set_port_in_use(id, service, owner, error.clone());
                        self.append_system_error(id, service, format!("Startup stopped because an external process owns the port: {error}"));
                        return;
                    }
                    {
                        let mut inner = self.lock();
                        let record = inner.records.entry(k.clone()).or_default();
                        record.state = ServiceState::Running;
                        record.port_owner = Some(owner.clone());
                        record.identity = Some(owner.process.clone());
                        record.pid = Some(owner.process.pid);
                        record.owned_pids.insert(owner.process.pid);
                        record.error = None;
                    }
                    self.append_system_log(id, service, "Azurite is listening".into());
                    self.emit_service_updated(id, service);
                    self.emit_snapshot_updated();
                    return;
                }
            }
            thread::sleep(Duration::from_millis(200));
        }
    }

    fn spawn_log_reader<R>(&self, id: String, service: ServiceName, stream: LogStream, reader: R)
    where
        R: std::io::Read + Send + 'static,
    {
        let engine = self.clone();
        thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => engine.append_log(&id, &service, stream.clone(), line),
                    Ok(_) => {}
                    Err(error) => {
                        engine.append_system_log(&id, &service, format!("log stream ended: {error}"));
                        break;
                    }
                }
            }
        });
    }

    fn append_log(&self, id: &str, service: &ServiceName, stream: LogStream, message: String) {
        let level = match stream {
            LogStream::Stdout => LogLevel::Info,
            LogStream::Stderr => LogLevel::Warn,
            LogStream::System => LogLevel::Info,
        };
        self.append_log_with_level(id, service, stream, level, message);
    }

    fn append_log_with_level(&self, id: &str, service: &ServiceName, stream: LogStream, level: LogLevel, message: String) {
        if stream == LogStream::System {
            logs::app_log(level.clone(), &format!("[{id}/{}] {message}", service_label(service)));
        }
        let entry = self.lock().logs.push_with_level(id, service.clone(), stream, level, message);
        self.emit(EngineEvent::LogEntry(entry));
    }

    fn append_system_log(&self, id: &str, service: &ServiceName, message: String) {
        self.append_log(id, service, LogStream::System, message);
    }

    fn append_system_error(&self, id: &str, service: &ServiceName, message: String) {
        self.append_log_with_level(id, service, LogStream::System, LogLevel::Error, message);
    }

    fn record_start_error(&self, id: &str, service: &ServiceName, error: String) {
        self.set_service_error(id, service, ServiceState::Broken, error.clone());
        self.append_system_error(id, service, error);
    }

    fn set_port_in_use(&self, id: &str, service: &ServiceName, owner: PortOwner, error: String) {
        {
            let mut inner = self.lock();
            let record = inner.records.entry(key(id, service)).or_default();
            record.state = ServiceState::PortInUse;
            record.port_owner = Some(owner);
            record.error = Some(error);
            record.exit_code = None;
        }
        self.emit_service_updated(id, service);
    }

    fn set_service_error(&self, id: &str, service: &ServiceName, state: ServiceState, error: String) {
        {
            let mut inner = self.lock();
            let record = inner.records.entry(key(id, service)).or_default();
            record.state = state;
            record.error = Some(error);
        }
        self.emit_service_updated(id, service);
    }

    fn mark_stopped(&self, id: &str, service: &ServiceName, exit_code: Option<i32>, error: Option<String>) {
        {
            let mut inner = self.lock();
            let record = inner.records.entry(key(id, service)).or_default();
            record.state = ServiceState::Stopped;
            record.stopped_at = Some(logs::now_timestamp());
            record.exit_code = exit_code;
            record.error = error;
            record.port_owner = None;
            record.identity = None;
            record.pid = None;
            record.started_instant = None;
            record.owned_pids.clear();
        }
        self.emit_service_updated(id, service);
    }

    /// Reap exited children, then reconcile every service with the OS listener
    /// table (one `netstat` for all instances).
    ///
    /// Returns true when any service's observable state changed.
    fn refresh_services(&self) -> bool {
        let before = self.fingerprint();
        self.refresh_services_inner();
        before != self.fingerprint()
    }

    fn fingerprint(&self) -> Vec<Fingerprint> {
        let inner = self.lock();
        inner
            .records
            .iter()
            .map(|(k, record)| {
                (
                    k.clone(),
                    record.state.clone(),
                    record.pid,
                    record.port_owner.as_ref().map(|owner| owner.process.pid),
                    record.exit_code,
                    record.error.clone(),
                )
            })
            .collect()
    }

    /// Background reaper: every few seconds, reap exited children and
    /// reconcile with the listener table, emitting a snapshot only on change.
    pub fn start_reaper(&self) {
        let engine = self.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(5));
            if engine.refresh_services() {
                let snapshot = snapshot_locked(&engine.lock());
                engine.emit(EngineEvent::SnapshotUpdated(snapshot));
            }
        });
    }

    fn refresh_services_inner(&self) {
        let keys: Vec<Key> = {
            let inner = self.lock();
            inner
                .config
                .instances
                .iter()
                .flat_map(|instance| SERVICES.iter().map(|service| key(&instance.id, service)))
                .collect()
        };

        for k in &keys {
            let child = self.lock().processes.get(k).map(|managed| managed.child.clone());
            let Some(child) = child else { continue };
            let status = child.lock().ok().and_then(|mut child| child.try_wait().ok().flatten());
            if let Some(status) = status {
                let code = status.code();
                self.lock().processes.remove(k);
                let error = (code != Some(0)).then(|| format!("Azurite exited with code {:?}", code));
                self.mark_stopped(&k.0, &k.1, code, error.clone());
                if let Some(error) = error {
                    self.append_system_error(&k.0, &k.1, error);
                }
            }
        }

        let table = ports::listeners().unwrap_or_default();
        type Target = (Key, String, u16, HashSet<u32>, HashMap<u32, ProcessIdentity>);
        let targets: Vec<Target> = {
            let inner = self.lock();
            keys.iter()
                .filter_map(|k| {
                    let instance = inner.config.instances.iter().find(|instance| instance.id == k.0)?;
                    let port = instance.ports.get(&k.1).copied().unwrap_or(0);
                    let record = inner.records.get(k);
                    let owned = record.map(|record| record.owned_pids.clone()).unwrap_or_default();
                    // Identities of app-owned pids are already recorded; reuse
                    // them instead of re-identifying through PowerShell.
                    let known: HashMap<u32, ProcessIdentity> = record
                        .and_then(|record| record.identity.clone())
                        .filter(|identity| owned.contains(&identity.pid))
                        .map(|identity| (identity.pid, identity))
                        .into_iter()
                        .collect();
                    Some((k.clone(), instance.host.clone(), port, owned, known))
                })
                .collect()
        };
        for (k, host, port, owned, known) in targets {
            let owner = ports::probe_in_cached(&table, &host, port, &owned, &known).owner;
            let mut inner = self.lock();
            let app_process_exists = inner.processes.contains_key(&k);
            let record = inner.records.entry(k).or_default();
            if app_process_exists {
                record.port_owner = owner.clone();
                if let Some(owner) = owner {
                    record.identity = Some(owner.process.clone());
                    record.pid = Some(owner.process.pid);
                }
            } else if owner.is_some() && record.state == ServiceState::Stopped {
                record.state = ServiceState::PortInUse;
                record.port_owner = owner;
                record.error = Some("a process already owns this port".into());
            } else if owner.is_none() && record.state == ServiceState::PortInUse {
                record.state = ServiceState::Stopped;
                record.port_owner = None;
                record.error = None;
            }
        }
    }

    fn emit_service_updated(&self, id: &str, service: &ServiceName) {
        let snapshot = {
            let inner = self.lock();
            let Some(instance) = inner.config.instances.iter().find(|instance| instance.id == id) else { return };
            service_snapshot(instance, inner.records.get(&key(id, service)), service)
        };
        self.emit(EngineEvent::ServiceUpdated(snapshot));
    }

    fn emit_snapshot_updated(&self) {
        self.emit(EngineEvent::SnapshotUpdated(self.snapshot()));
    }

    fn emit(&self, event: EngineEvent) {
        let handler = self.lock().event_handler.clone();
        if let Some(handler) = handler {
            handler(event);
        }
    }
}

impl Default for ServiceRecord {
    fn default() -> Self {
        Self {
            state: ServiceState::Stopped,
            started_at: None,
            started_instant: None,
            stopped_at: None,
            pid: None,
            identity: None,
            port_owner: None,
            exit_code: None,
            error: None,
            owned_pids: HashSet::new(),
        }
    }
}

fn snapshot_locked(inner: &EngineInner) -> AppSnapshot {
    let instances = inner
        .config
        .instances
        .iter()
        .map(|instance| instance_snapshot(inner, instance))
        .collect();
    AppSnapshot {
        app: AppInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            features: vec!["mcp".to_string(), "multiInstance".to_string()],
            config_path: config::config_path().to_string_lossy().into_owned(),
            log_path: config::app_log_path().to_string_lossy().into_owned(),
            config_error: inner.config_error.clone(),
        },
        config: inner.config.clone(),
        engine: inner.engine.clone(),
        mcp: inner.mcp_status.clone(),
        instances,
        generated_at: logs::now_timestamp(),
    }
}

fn instance_snapshot(inner: &EngineInner, instance: &InstanceConfig) -> InstanceSnapshot {
    let services: BTreeMap<ServiceName, ServiceSnapshot> = SERVICES
        .iter()
        .map(|service| {
            (
                service.clone(),
                service_snapshot(instance, inner.records.get(&key(&instance.id, service)), service),
            )
        })
        .collect();
    let states: Vec<ServiceState> = services.values().map(|service| service.state.clone()).collect();
    let (logs, merged_logs) = inner.logs.instance_logs(&instance.id, SNAPSHOT_SERVICE_LOGS, SNAPSHOT_MERGED_LOGS);
    InstanceSnapshot {
        config: instance.clone(),
        state: derive_instance_state(&states),
        services,
        connection: build_connection_info(instance),
        logs,
        merged_logs,
    }
}

fn service_snapshot(instance: &InstanceConfig, record: Option<&ServiceRecord>, service: &ServiceName) -> ServiceSnapshot {
    let default_record = ServiceRecord::default();
    let record = record.unwrap_or(&default_record);
    ServiceSnapshot {
        instance_id: instance.id.clone(),
        name: service.clone(),
        state: record.state.clone(),
        host: instance.host.clone(),
        port: instance.ports.get(service).copied().unwrap_or(0),
        pid: record.pid,
        process_identity: record.identity.clone(),
        port_owner: record.port_owner.clone(),
        started_at: record.started_at.clone(),
        stopped_at: record.stopped_at.clone(),
        uptime_seconds: record.started_instant.map(|instant| instant.elapsed().as_secs()),
        exit_code: record.exit_code,
        error: record.error.clone(),
    }
}

fn engine_snapshot_for_config(config: &AppConfig, error: Option<String>) -> EngineSnapshot {
    EngineSnapshot { state: EngineState::InvalidConfig, node_path: config.node_path.clone(), azurite_path: config.executable_path.clone(), node_version: None, azurite_version: None, message: error, install_hint: Some("Fix AzTray's config.json or remove it to restore defaults.".into()) }
}

fn inspect_engine(config: &AppConfig, config_error: Option<String>) -> EngineSnapshot {
    if let Some(message) = config_error {
        return engine_snapshot_for_config(config, Some(message));
    }
    let node = config.node_path.as_deref().unwrap_or("node");
    let Some(node_version) = command_version(node) else {
        return EngineSnapshot { state: EngineState::MissingNode, node_path: Some(node.into()), azurite_path: config.executable_path.clone(), node_version: None, azurite_version: None, message: Some("Node.js could not be found.".into()), install_hint: Some("Install Node.js from nodejs.org or set the Node path in Settings.".into()) };
    };
    let azurite = resolve_azurite_probe(config);
    let Some((azurite_path, azurite_version)) = azurite else {
        return EngineSnapshot { state: EngineState::MissingAzurite, node_path: Some(node.into()), azurite_path: config.executable_path.clone(), node_version: Some(node_version), azurite_version: None, message: Some("Azurite could not be found.".into()), install_hint: Some("Run npm install --global azurite, or set an Azurite executable path.".into()) };
    };
    EngineSnapshot { state: EngineState::Ready, node_path: Some(node.into()), azurite_path: Some(azurite_path), node_version: Some(node_version), azurite_version: Some(azurite_version), message: None, install_hint: None }
}

fn resolve_launch_spec(config: &AppConfig, instance: &InstanceConfig, service: &ServiceName) -> Result<LaunchSpec, String> {
    let label = service_label(service);
    let mut args = azurite_args(instance, service)?;
    if let Some(path) = config.executable_path.as_deref() {
        let path = PathBuf::from(path);
        let candidate = if path.is_dir() { find_in_directory(&path, label).ok_or_else(|| format!("Azurite executable was not found under {}", path.display()))? } else { path };
        if candidate.extension().and_then(|extension| extension.to_str()).map(|extension| extension.eq_ignore_ascii_case("js")).unwrap_or(false) {
            let node = config.node_path.as_deref().unwrap_or("node");
            return Ok(LaunchSpec { program: node.into(), args: { let mut values = vec![candidate.to_string_lossy().into_owned()]; values.append(&mut args); values }, display: candidate.to_string_lossy().into_owned() });
        }
        return Ok(LaunchSpec { program: candidate.to_string_lossy().into_owned(), args, display: candidate.to_string_lossy().into_owned() });
    }
    let binary = format!("azurite-{label}");
    if let Some(program) = resolve_command_path(&binary) {
        return Ok(LaunchSpec { program, args, display: binary });
    }
    if let Some(program) = resolve_command_path("npx") {
        let mut npx_args = vec!["--no-install".into(), binary.clone()];
        npx_args.append(&mut args);
        return Ok(LaunchSpec { program, args: npx_args, display: format!("npx --no-install {binary}") });
    }
    Err("Azurite service executable is missing; install Azurite or configure its path".into())
}

fn resolve_azurite_probe(config: &AppConfig) -> Option<(String, String)> {
    if let Some(path) = config.executable_path.as_deref() {
        let path = PathBuf::from(path);
        let candidate = if path.is_dir() { find_in_directory(&path, "blob")? } else { path };
        let version = command_version(candidate.to_string_lossy().as_ref())?;
        return Some((candidate.to_string_lossy().into_owned(), version));
    }
    for binary in ["azurite-blob", "azurite"] {
        if let Some(program) = resolve_command_path(binary) {
            let version = command_version(&program)?;
            return Some((program, version));
        }
    }
    if let Some(program) = resolve_command_path("npx") {
        let mut command = Command::new(&program);
        command.args(["--no-install", "azurite-blob", "--version"]);
        hide_console(&mut command);
        if let Ok(output) = command.output() {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !version.is_empty() {
                    return Some(("npx --no-install azurite-blob".into(), version));
                }
            }
        }
    }
    None
}

fn find_in_directory(directory: &Path, label: &str) -> Option<PathBuf> {
    let names = [format!("azurite-{label}.cmd"), format!("azurite-{label}.exe"), format!("azurite-{label}"), "azurite.cmd".into(), "azurite.exe".into(), "azurite".into()];
    names.iter().map(|name| directory.join(name)).find(|candidate| candidate.is_file())
}

fn command_version(program: &str) -> Option<String> {
    let program = resolve_command_path(program)?;
    let mut command = Command::new(&program);
    command.arg("--version");
    hide_console(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() { return None; }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn resolve_command_path(program: &str) -> Option<String> {
    if Path::new(program).is_file() {
        return Some(program.to_string());
    }

    #[cfg(windows)]
    {
        let mut command = Command::new("where");
        command.arg(program);
        hide_console(&mut command);
        let output = command.output().ok()?;
        if !output.status.success() {
            return None;
        }
        select_command_path(&String::from_utf8_lossy(&output.stdout))
    }

    #[cfg(not(windows))]
    {
        let mut command = Command::new("sh");
        command.args(["-c", "command -v \"$1\" >/dev/null 2>&1", "aztray", program]);
        hide_console(&mut command);
        command.output().ok()?.status.success().then_some(program.to_string())
    }
}

#[cfg(windows)]
fn select_command_path(output: &str) -> Option<String> {
    let lines: Vec<String> = output.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect();
    lines.iter().find(|line| {
        Path::new(line).extension().and_then(|extension| extension.to_str()).map(|extension| extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("cmd")).unwrap_or(false)
    }).cloned().or_else(|| lines.into_iter().next())
}

fn wait_for_exit(child: &Arc<Mutex<Child>>, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if child.lock().ok().and_then(|mut child| child.try_wait().ok()).flatten().is_some() { return; }
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_pid_exit(pid: u32, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !ports::process_alive(pid) { return; }
        thread::sleep(Duration::from_millis(100));
    }
}

fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
}

fn service_label(service: &ServiceName) -> &'static str {
    config::service_label(service)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn missing_configured_executable_is_not_reported_as_available() {
        let mut config = config::default_config();
        let missing_path = std::env::temp_dir().join(format!(
            "aztray-missing-executable-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).expect("clock is after epoch").as_nanos()
        ));
        assert!(!missing_path.exists(), "test path unexpectedly exists: {}", missing_path.display());
        config.executable_path = Some(missing_path.to_string_lossy().into_owned());

        assert_eq!(resolve_azurite_probe(&config), None);
    }

    #[cfg(windows)]
    #[test]
    fn where_output_prefers_windows_shim_over_extensionless_match() {
        let output = "C:\\nvm4w\\nodejs\\npx\r\nC:\\nvm4w\\nodejs\\npx.cmd\r\n";
        assert_eq!(select_command_path(output), Some("C:\\nvm4w\\nodejs\\npx.cmd".into()));
    }

    #[cfg(windows)]
    #[test]
    fn full_path_cmd_shim_can_be_version_probed() {
        let path = std::env::temp_dir().join(format!(
            "aztray-version-probe-{}-{}.cmd",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).expect("clock is after epoch").as_nanos()
        ));
        fs::write(&path, "@echo off\r\necho 9.9.9\r\n").expect("write cmd shim");

        assert_eq!(command_version(path.to_string_lossy().as_ref()), Some("9.9.9".into()));

        let _ = fs::remove_file(path);
    }

    fn instance(id: &str, name: &str) -> InstanceConfig {
        InstanceConfig { id: id.into(), name: name.into(), ..config::default_instance() }
    }

    fn test_engine() -> AppEngine {
        let config = config::default_config();
        let engine = engine_snapshot_for_config(&config, None);
        let mcp_status = McpStatus::initial(&config.mcp);
        AppEngine {
            inner: Arc::new(Mutex::new(EngineInner {
                config,
                config_error: None,
                engine,
                records: BTreeMap::new(),
                processes: BTreeMap::new(),
                starting: BTreeSet::new(),
                logs: LogStore::default(),
                mcp_status,
                event_handler: None,
            })),
        }
    }

    fn exited_child() -> Child {
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd");
            command.args(["/C", "exit", "0"]);
            command
        };
        #[cfg(not(windows))]
        let mut command = Command::new("true");
        hide_console(&mut command);
        let mut child = command.stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("spawn short-lived child");
        child.wait().expect("child exits");
        child
    }

    #[test]
    fn concurrent_starts_reserve_the_key_exactly_once() {
        let engine = test_engine();
        let k = key("default", &ServiceName::Blob);
        let winners: usize = thread::scope(|scope| {
            let handles: Vec<_> = (0..16)
                .map(|_| scope.spawn(|| engine.try_reserve_start(&k)))
                .collect();
            handles.into_iter().map(|handle| handle.join().unwrap()).filter(|won| *won).count()
        });
        assert_eq!(winners, 1, "exactly one concurrent start may proceed");
        // Other keys are independent.
        assert!(engine.try_reserve_start(&key("default", &ServiceName::Queue)));
        // Releasing the reservation lets a later start proceed.
        engine.lock().starting.remove(&k);
        assert!(engine.try_reserve_start(&k));
    }

    #[test]
    fn reservation_refuses_when_a_process_is_already_managed() {
        let engine = test_engine();
        let k = key("default", &ServiceName::Table);
        let child = exited_child();
        let root_pid = child.id();
        engine.lock().processes.insert(k.clone(), ManagedProcess { child: Arc::new(Mutex::new(child)), root_pid });
        assert!(!engine.try_reserve_start(&k));
        assert!(!engine.lock().starting.contains(&k));
    }

    #[test]
    fn stopping_an_already_dead_process_marks_it_stopped() {
        let engine = test_engine();
        let k = key("default", &ServiceName::Blob);
        let child = exited_child();
        let root_pid = child.id();
        {
            let mut inner = engine.lock();
            inner.processes.insert(k.clone(), ManagedProcess { child: Arc::new(Mutex::new(child)), root_pid });
            let record = inner.records.entry(k.clone()).or_default();
            record.state = ServiceState::Running;
            record.pid = Some(root_pid);
        }
        engine.stop_one("default", &ServiceName::Blob).expect("stopping a dead process succeeds");
        let inner = engine.lock();
        assert!(inner.processes.is_empty());
        assert_eq!(inner.records[&k].state, ServiceState::Stopped);
        assert_eq!(inner.records[&k].pid, None);
    }

    fn config_of(instances: Vec<InstanceConfig>) -> AppConfig {
        AppConfig { instances, ..config::default_config() }
    }

    #[test]
    fn selector_resolution_follows_default_rules() {
        let both = config_of(vec![instance("default", "Default"), instance("dev", "Dev Box")]);
        assert_eq!(resolve_selector(&both, None).unwrap(), "default");
        assert_eq!(resolve_selector(&both, Some("  ")).unwrap(), "default");
        assert_eq!(resolve_selector(&both, Some("dev")).unwrap(), "dev");
        assert_eq!(resolve_selector(&both, Some("DEV BOX")).unwrap(), "dev");
        let err = resolve_selector(&both, Some("nope")).unwrap_err();
        assert_eq!(err, "no instance matches \"nope\". Available: default (\"Default\"), dev (\"Dev Box\")");

        let sole = config_of(vec![instance("dev", "Dev")]);
        assert_eq!(resolve_selector(&sole, None).unwrap(), "dev");

        let many = config_of(vec![instance("a", "A"), instance("b", "B")]);
        let err = resolve_selector(&many, None).unwrap_err();
        assert_eq!(err, "multiple instances; specify instance. Available: a (\"A\"), b (\"B\")");
    }

    #[test]
    fn id_takes_precedence_over_a_conflicting_name() {
        let config = config_of(vec![instance("a", "b"), instance("b", "Other")]);
        assert_eq!(resolve_selector(&config, Some("b")).unwrap(), "b");
    }

    #[test]
    fn allocation_skips_configured_and_mcp_ports() {
        let config = config::default_config();
        let taken = taken_ports(&config);
        for port in [10000u16, 10001, 10002, 47551, 47560] {
            assert!(taken.contains(&port), "{port}");
        }
        assert!(!taken.contains(&47561));
        let ports = allocate_ports(&config, "127.0.0.1").expect("trio");
        let blob = ports[&ServiceName::Blob];
        assert!(blob >= 10003 && (blob - 10000).is_multiple_of(3));
        assert_eq!(ports[&ServiceName::Queue], blob + 1);
        assert_eq!(ports[&ServiceName::Table], blob + 2);
    }

    #[test]
    fn instance_state_aggregation() {
        use ServiceState::*;
        assert_eq!(derive_instance_state(&[Stopped, Stopped, Stopped]), InstanceState::Stopped);
        assert_eq!(derive_instance_state(&[Running, Running, Running]), InstanceState::Running);
        assert_eq!(derive_instance_state(&[Running, Stopped, Running]), InstanceState::Partial);
        assert_eq!(derive_instance_state(&[Running, Starting, Stopped]), InstanceState::Starting);
        assert_eq!(derive_instance_state(&[Running, PortInUse, Starting]), InstanceState::Starting);
        assert_eq!(derive_instance_state(&[Running, PortInUse, Running]), InstanceState::Broken);
    }

    #[test]
    fn connection_info_maps_wildcard_host_and_ports() {
        let mut inst = instance("dev", "Dev");
        inst.host = "0.0.0.0".into();
        inst.ports = [(ServiceName::Blob, 10003), (ServiceName::Queue, 10004), (ServiceName::Table, 10005)].into_iter().collect();
        let info = build_connection_info(&inst);
        assert_eq!(info.endpoints[&ServiceName::Queue], "http://127.0.0.1:10004/devstoreaccount1");
        assert!(info.connection_strings[&ServiceName::Table].ends_with("TableEndpoint=http://127.0.0.1:10005/devstoreaccount1;"));
        let combined = &info.connection_string;
        let (b, q, t) = (combined.find("BlobEndpoint").unwrap(), combined.find("QueueEndpoint").unwrap(), combined.find("TableEndpoint").unwrap());
        assert!(b < q && q < t);
        assert!(combined.contains("AccountName=devstoreaccount1;"));
    }

    #[test]
    fn azurite_args_include_instance_flags() {
        let mut inst = instance("dev", "Dev");
        inst.loose = true;
        inst.skip_api_version_check = true;
        inst.data_directory = r"C:\d".into();
        let args = azurite_args(&inst, &ServiceName::Queue).unwrap();
        assert_eq!(&args[..4], ["--queueHost", "127.0.0.1", "--queuePort", "10001"]);
        assert!(args.contains(&"--loose".to_string()) && args.contains(&"--skipApiVersionCheck".to_string()));
        let plain = azurite_args(&instance("x", "X"), &ServiceName::Blob).unwrap();
        assert!(!plain.contains(&"--loose".to_string()));
    }
}
