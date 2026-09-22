use crate::config;
use crate::logs::{self, LogStore};
use crate::ports;
use crate::types::*;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const AZURITE_ACCOUNT_KEY: &str = "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";

pub type EventHandler = Arc<dyn Fn(EngineEvent) + Send + Sync + 'static>;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum EngineEvent {
    SnapshotUpdated(AppSnapshot),
    ServiceUpdated(ServiceSnapshot),
    LogEntry(LogEntry),
    EngineUpdated(EngineSnapshot),
}

#[derive(Clone)]
pub struct AppEngine {
    inner: Arc<Mutex<EngineInner>>,
}

struct EngineInner {
    config: Config,
    config_error: Option<String>,
    engine: EngineSnapshot,
    services: BTreeMap<ServiceName, ServiceRecord>,
    processes: BTreeMap<ServiceName, ManagedProcess>,
    logs: LogStore,
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

struct ManagedProcess {
    child: Arc<Mutex<Child>>,
    root_pid: u32,
}

struct LaunchSpec {
    program: String,
    args: Vec<String>,
    display: String,
}

impl AppEngine {
    pub fn new() -> Self {
        let loaded = config::load();
        let mut services = BTreeMap::new();
        for service in [ServiceName::Blob, ServiceName::Queue, ServiceName::Table] {
            services.insert(service, ServiceRecord::default());
        }
        let engine = inspect_engine(&loaded.config, loaded.error.clone());
        Self {
            inner: Arc::new(Mutex::new(EngineInner {
                config: loaded.config,
                config_error: loaded.error,
                engine,
                services,
                processes: BTreeMap::new(),
                logs: LogStore::default(),
                event_handler: None,
            })),
        }
    }

    pub fn set_event_handler(&self, handler: Option<EventHandler>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.event_handler = handler;
        }
    }

    pub fn snapshot(&self) -> AppSnapshot {
        self.refresh_services();
        let inner = self.inner.lock().expect("AzTray engine mutex poisoned");
        snapshot_locked(&inner)
    }

    pub fn get_snapshot(&self) -> AppSnapshot {
        self.snapshot()
    }

    pub fn set_config(&self, next: Config) -> Result<AppSnapshot, String> {
        config::validate(&next)?;
        {
            let inner = self.inner.lock().map_err(|_| "engine lock poisoned".to_string())?;
            if !inner.processes.is_empty() {
                return Err("stop Azurite services before changing configuration".into());
            }
        }
        config::persist(&next)?;
        {
            let mut inner = self.inner.lock().map_err(|_| "engine lock poisoned".to_string())?;
            inner.config = next;
            inner.config_error = None;
            inner.engine = engine_snapshot_for_config(&inner.config, None);
        }
        self.check_engine();
        let snapshot = self.snapshot();
        self.emit(EngineEvent::SnapshotUpdated(snapshot.clone()));
        Ok(snapshot)
    }

    pub fn check_engine(&self) -> EngineSnapshot {
        let (config, config_error) = {
            let inner = self.inner.lock().expect("AzTray engine mutex poisoned");
            (inner.config.clone(), inner.config_error.clone())
        };
        let snapshot = inspect_engine(&config, config_error);
        if let Ok(mut inner) = self.inner.lock() {
            inner.engine = snapshot.clone();
        }
        self.emit(EngineEvent::EngineUpdated(snapshot.clone()));
        snapshot
    }

    pub fn start_service(&self, service: ServiceName) -> Result<AppSnapshot, String> {
        let engine = self.check_engine();
        if engine.state != EngineState::Ready {
            return Err(engine.message.unwrap_or_else(|| "Azurite is not ready".into()));
        }
        let (config, port, existing_process) = {
            let inner = self.inner.lock().map_err(|_| "engine lock poisoned".to_string())?;
            let port = *inner.config.ports.get(&service).ok_or_else(|| "service port is missing".to_string())?;
            (inner.config.clone(), port, inner.processes.contains_key(&service))
        };
        if existing_process {
            let snapshot = self.snapshot();
            self.emit(EngineEvent::SnapshotUpdated(snapshot.clone()));
            return Ok(snapshot);
        }

        let owned_pids = HashSet::new();
        let probe = ports::probe(&config.host, port, &owned_pids);
        if let Some(owner) = probe.owner {
            self.set_port_in_use(&service, owner.clone(), "service port is already in use".into());
            return Err(format!("{} port {} is used by PID {}", service_label(&service), port, owner.process.pid));
        }
        if let Some(error) = probe.error {
            self.set_service_error(&service, ServiceState::Broken, error.clone());
            return Err(error);
        }
        std::fs::create_dir_all(&config.data_directory)
            .map_err(|error| format!("could not create Azurite data directory: {error}"))?;
        let spec = resolve_launch_spec(&config, &service)?;
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide_console(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| format!("could not start {}: {error}", spec.display))?;
        let root_pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));
        let started_at = logs::now_timestamp();
        {
            let mut inner = self.inner.lock().map_err(|_| "engine lock poisoned".to_string())?;
            let record = inner.services.get_mut(&service).expect("service record exists");
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
            inner.processes.insert(service.clone(), ManagedProcess { child: child.clone(), root_pid });
        }
        self.append_system_log(&service, format!("Starting {spec_display}", spec_display = spec.display));
        self.emit_service_updated(&service);
        if let Some(stdout) = stdout {
            self.spawn_log_reader(service.clone(), LogStream::Stdout, stdout);
        }
        if let Some(stderr) = stderr {
            self.spawn_log_reader(service.clone(), LogStream::Stderr, stderr);
        }
        let watcher = self.clone();
        thread::spawn(move || watcher.watch_start(service));
        let snapshot = self.snapshot();
        self.emit(EngineEvent::SnapshotUpdated(snapshot.clone()));
        Ok(snapshot)
    }

    pub fn stop_service(&self, service: ServiceName) -> Result<AppSnapshot, String> {
        let managed = {
            let mut inner = self.inner.lock().map_err(|_| "engine lock poisoned".to_string())?;
            inner.processes.remove(&service)
        };
        let Some(managed) = managed else {
            self.refresh_services();
            let snapshot = self.snapshot();
            self.emit(EngineEvent::SnapshotUpdated(snapshot.clone()));
            return Ok(snapshot);
        };
        ports::terminate(managed.root_pid, true)?;
        wait_for_exit(&managed.child, Duration::from_secs(2));
        self.mark_stopped(&service, None, None);
        self.append_system_log(&service, "Stopped by user".into());
        let snapshot = self.snapshot();
        self.emit(EngineEvent::SnapshotUpdated(snapshot.clone()));
        Ok(snapshot)
    }

    pub fn restart_service(&self, service: ServiceName) -> Result<AppSnapshot, String> {
        self.stop_service(service.clone())?;
        self.start_service(service)
    }

    pub fn start_all(&self) -> Result<AppSnapshot, String> {
        let mut first_error = None;
        for service in [ServiceName::Blob, ServiceName::Queue, ServiceName::Table] {
            if let Err(error) = self.start_service(service) {
                first_error.get_or_insert(error);
            }
        }
        let snapshot = self.snapshot();
        first_error.map_or(Ok(snapshot), Err)
    }

    pub fn stop_all(&self) -> Result<AppSnapshot, String> {
        let mut first_error = None;
        for service in [ServiceName::Blob, ServiceName::Queue, ServiceName::Table] {
            if let Err(error) = self.stop_service(service) {
                first_error.get_or_insert(error);
            }
        }
        let snapshot = self.snapshot();
        first_error.map_or(Ok(snapshot), Err)
    }

    pub fn restart_all(&self) -> Result<AppSnapshot, String> {
        self.stop_all()?;
        self.start_all()
    }

    pub fn identify_port_owner(&self, service: ServiceName) -> Option<PortOwner> {
        self.refresh_services();
        let inner = self.inner.lock().ok()?;
        let port = *inner.config.ports.get(&service)?;
        let owned_pids = inner.services.get(&service).map(|record| record.owned_pids.clone()).unwrap_or_default();
        ports::probe(&inner.config.host, port, &owned_pids).owner
    }

    pub fn free_port(&self, expected: PortOwnerExpectation) -> Result<FreePortResult, String> {
        self.free_port_confirmed(expected, true)
    }

    pub fn free_port_confirmed(&self, expected: PortOwnerExpectation, confirmed: bool) -> Result<FreePortResult, String> {
        if !confirmed {
            return Err("free port requires explicit confirmation".into());
        }
        let current = self.identify_port_owner(expected.service_name.clone());
        let Some(owner) = current else {
            return Ok(FreePortResult { snapshot: self.snapshot(), released: false, surviving_owner: None, message: "The port is already free.".into() });
        };
        if !ports::identity_matches(&owner, expected.pid, expected.started_at.as_deref()) {
            return Err("port owner changed; refusing to terminate a reused PID".into());
        }
        if owner.owned_by_app {
            self.stop_service(expected.service_name.clone())?;
        } else {
            ports::terminate(owner.process.pid, true)?;
            wait_for_pid_exit(owner.process.pid, Duration::from_secs(2));
        }
        let surviving_owner = self.identify_port_owner(expected.service_name.clone());
        let released = surviving_owner.is_none();
        let message = if released { "Port released." } else { "The process survived or respawned." };
        let snapshot = self.snapshot();
        self.emit(EngineEvent::SnapshotUpdated(snapshot.clone()));
        Ok(FreePortResult { snapshot, released, surviving_owner, message: message.into() })
    }

    pub fn get_logs(&self, query: LogsQuery) -> Vec<LogEntry> {
        self.inner.lock().map(|inner| inner.logs.query(&query)).unwrap_or_default()
    }

    pub fn save_logs(&self, args: SaveLogsArgs) -> Result<SaveLogsResult, String> {
        self.inner
            .lock()
            .map_err(|_| "engine lock poisoned".to_string())?
            .logs
            .save(args.service_name.as_ref(), args.path.as_deref())
    }

    pub fn connection_string(&self, service: ServiceName) -> String {
        let inner = self.inner.lock().expect("AzTray engine mutex poisoned");
        let port = inner.config.ports.get(&service).copied().unwrap_or(0);
        let endpoint = format!("http://{}:{}/devstoreaccount1", inner.config.host, port);
        match service {
            ServiceName::Blob => format!("DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey={AZURITE_ACCOUNT_KEY};BlobEndpoint={endpoint};"),
            ServiceName::Queue => format!("DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey={AZURITE_ACCOUNT_KEY};QueueEndpoint={endpoint};"),
            ServiceName::Table => format!("DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey={AZURITE_ACCOUNT_KEY};TableEndpoint={endpoint};"),
        }
    }

    pub fn clear_logs(&self, service: Option<ServiceName>) -> AppSnapshot {
        if let Ok(mut inner) = self.inner.lock() {
            inner.logs.clear(service.as_ref());
        }
        let snapshot = self.snapshot();
        self.emit(EngineEvent::SnapshotUpdated(snapshot.clone()));
        snapshot
    }

    pub fn quit(&self, mode: QuitMode) -> Result<QuitResult, String> {
        if mode == QuitMode::Cancel {
            return Ok(QuitResult { mode, stopped_services: Vec::new() });
        }
        let mut stopped_services = Vec::new();
        if mode == QuitMode::StopAndQuit {
            for service in [ServiceName::Blob, ServiceName::Queue, ServiceName::Table] {
                let owned = self.inner.lock().ok().and_then(|inner| inner.processes.get(&service).map(|_| true)).unwrap_or(false);
                if owned && self.stop_service(service.clone()).is_ok() {
                    stopped_services.push(service);
                }
            }
        }
        Ok(QuitResult { mode, stopped_services })
    }

    fn watch_start(&self, service: ServiceName) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if Instant::now() >= deadline {
                let managed = self.inner.lock().ok().and_then(|mut inner| inner.processes.remove(&service));
                if let Some(managed) = managed {
                    let _ = ports::terminate(managed.root_pid, true);
                }
                self.set_service_error(&service, ServiceState::Broken, "Azurite did not begin listening within 10 seconds".into());
                self.append_system_log(&service, "Startup timed out after 10 seconds".into());
                return;
            }
            let (config, port, managed, root_pid) = match self.inner.lock() {
                Ok(inner) => {
                    let Some(managed) = inner.processes.get(&service) else { return };
                    (inner.config.clone(), *inner.config.ports.get(&service).unwrap_or(&0), managed.child.clone(), managed.root_pid)
                }
                Err(_) => return,
            };
            if let Ok(mut child) = managed.lock() {
                if let Ok(Some(status)) = child.try_wait() {
                    self.inner.lock().ok().map(|mut inner| inner.processes.remove(&service));
                    self.set_service_error(&service, ServiceState::Broken, format!("Azurite exited during startup ({status})"));
                    return;
                }
            }
            let owned = self.inner.lock().ok().and_then(|inner| inner.services.get(&service).map(|record| record.owned_pids.clone())).unwrap_or_default();
            if let Some(owner) = ports::probe(&config.host, port, &owned).owner {
                if owner.process.pid != 0 {
                    if !ports::is_descendant_or_self(owner.process.pid, root_pid) {
                        let managed = self.inner.lock().ok().and_then(|mut inner| inner.processes.remove(&service));
                        if let Some(managed) = managed {
                            let _ = ports::terminate(managed.root_pid, true);
                        }
                        self.set_port_in_use(&service, owner, "another process claimed the service port during startup".into());
                        self.append_system_log(&service, "Startup stopped because an external process owns the port".into());
                        return;
                    }
                    if let Ok(mut inner) = self.inner.lock() {
                        let record = inner.services.get_mut(&service).expect("service record exists");
                        record.state = ServiceState::Running;
                        record.port_owner = Some(owner.clone());
                        record.identity = Some(owner.process.clone());
                        record.pid = Some(owner.process.pid);
                        record.owned_pids.insert(owner.process.pid);
                        record.error = None;
                    }
                    self.append_system_log(&service, "Azurite is listening".into());
                    self.emit_service_updated(&service);
                    self.emit_snapshot_updated();
                    return;
                }
            }
            thread::sleep(Duration::from_millis(200));
        }
    }

    fn spawn_log_reader<R>(&self, service: ServiceName, stream: LogStream, reader: R)
    where
        R: std::io::Read + Send + 'static,
    {
        let engine = self.clone();
        thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => engine.append_log(service.clone(), stream.clone(), line),
                    Ok(_) => {}
                    Err(error) => {
                        engine.append_system_log(&service, format!("log stream ended: {error}"));
                        break;
                    }
                }
            }
        });
    }

    fn append_log(&self, service: ServiceName, stream: LogStream, message: String) {
        let entry = match self.inner.lock() {
            Ok(mut inner) => inner.logs.push(service, stream, message),
            Err(_) => return,
        };
        self.emit(EngineEvent::LogEntry(entry));
    }

    fn append_system_log(&self, service: &ServiceName, message: String) {
        self.append_log(service.clone(), LogStream::System, message);
    }

    fn set_port_in_use(&self, service: &ServiceName, owner: PortOwner, error: String) {
        if let Ok(mut inner) = self.inner.lock() {
            let record = inner.services.get_mut(service).expect("service record exists");
            record.state = ServiceState::PortInUse;
            record.port_owner = Some(owner);
            record.error = Some(error);
            record.exit_code = None;
        }
        self.emit_service_updated(service);
    }

    fn set_service_error(&self, service: &ServiceName, state: ServiceState, error: String) {
        if let Ok(mut inner) = self.inner.lock() {
            let record = inner.services.get_mut(service).expect("service record exists");
            record.state = state;
            record.error = Some(error);
        }
        self.emit_service_updated(service);
    }

    fn mark_stopped(&self, service: &ServiceName, exit_code: Option<i32>, error: Option<String>) {
        if let Ok(mut inner) = self.inner.lock() {
            let record = inner.services.get_mut(service).expect("service record exists");
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
        self.emit_service_updated(service);
    }

    fn refresh_services(&self) {
        let services = [ServiceName::Blob, ServiceName::Queue, ServiceName::Table];
        for service in services {
            let process = self.inner.lock().ok().and_then(|inner| inner.processes.get(&service).map(|managed| managed.child.clone()));
            if let Some(process) = process {
                if let Ok(mut child) = process.lock() {
                    if let Ok(Some(status)) = child.try_wait() {
                        let code = status.code();
                        self.inner.lock().ok().map(|mut inner| inner.processes.remove(&service));
                        self.mark_stopped(&service, code, (code != Some(0)).then(|| format!("Azurite exited with code {:?}", code)));
                        continue;
                    }
                }
            }
            let config = match self.inner.lock() {
                Ok(inner) => inner.config.clone(),
                Err(_) => continue,
            };
            let owned = self.inner.lock().ok().and_then(|inner| inner.services.get(&service).map(|record| record.owned_pids.clone())).unwrap_or_default();
            let port = config.ports.get(&service).copied().unwrap_or(0);
            let owner = ports::probe(&config.host, port, &owned).owner;
            if let Ok(mut inner) = self.inner.lock() {
                let app_process_exists = inner.processes.contains_key(&service);
                let record = inner.services.get_mut(&service).expect("service record exists");
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
    }

    fn emit_service_updated(&self, service: &ServiceName) {
        let snapshot = self.snapshot_service(service);
        self.emit(EngineEvent::ServiceUpdated(snapshot));
    }

    fn snapshot_service(&self, service: &ServiceName) -> ServiceSnapshot {
        self.refresh_service_record(service);
        let inner = self.inner.lock().expect("AzTray engine mutex poisoned");
        service_snapshot(&inner, service)
    }

    fn refresh_service_record(&self, _service: &ServiceName) {}

    fn emit_snapshot_updated(&self) {
        self.emit(EngineEvent::SnapshotUpdated(self.snapshot()));
    }

    fn emit(&self, event: EngineEvent) {
        let handler = self.inner.lock().ok().and_then(|inner| inner.event_handler.clone());
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
    let services = inner.services.keys().map(|service| (service.clone(), service_snapshot(inner, service))).collect();
    let (logs, merged_logs) = inner.logs.all();
    AppSnapshot {
        config: inner.config.clone(),
        engine: inner.engine.clone(),
        services,
        logs,
        merged_logs,
        generated_at: logs::now_timestamp(),
    }
}

fn service_snapshot(inner: &EngineInner, service: &ServiceName) -> ServiceSnapshot {
    let record = inner.services.get(service).expect("service record exists");
    let port = inner.config.ports.get(service).copied().unwrap_or(0);
    ServiceSnapshot {
        name: service.clone(),
        state: record.state.clone(),
        host: inner.config.host.clone(),
        port,
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

fn engine_snapshot_for_config(config: &Config, error: Option<String>) -> EngineSnapshot {
    if let Some(message) = error {
        return EngineSnapshot { state: EngineState::InvalidConfig, node_path: config.node_path.clone(), azurite_path: config.executable_path.clone(), node_version: None, azurite_version: None, message: Some(message), install_hint: Some("Fix AzTray's config.json or remove it to restore defaults.".into()) };
    }
    EngineSnapshot { state: EngineState::MissingNode, node_path: config.node_path.clone(), azurite_path: config.executable_path.clone(), node_version: None, azurite_version: None, message: Some("Engine availability has not been checked yet.".into()), install_hint: Some("Install Node.js, then install Azurite with npm install --global azurite.".into()) }
}

fn inspect_engine(config: &Config, config_error: Option<String>) -> EngineSnapshot {
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

fn resolve_launch_spec(config: &Config, service: &ServiceName) -> Result<LaunchSpec, String> {
    let label = service_label(service);
    let port = config.ports.get(service).copied().ok_or_else(|| format!("missing {label} port"))?;
    let mut args = vec![format!("--{}Host", label), config.host.clone(), format!("--{}Port", label), port.to_string(), "--location".into(), config.data_directory.clone()];
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
    if command_available(&binary) {
        return Ok(LaunchSpec { program: binary.clone(), args, display: binary });
    }
    if command_available("npx") {
        let mut npx_args = vec!["--no-install".into(), binary.clone()];
        npx_args.append(&mut args);
        return Ok(LaunchSpec { program: "npx".into(), args: npx_args, display: format!("npx --no-install {binary}") });
    }
    Err("Azurite service executable is missing; install Azurite or configure its path".into())
}

fn resolve_azurite_probe(config: &Config) -> Option<(String, String)> {
    if let Some(path) = config.executable_path.as_deref() {
        let path = PathBuf::from(path);
        let candidate = if path.is_dir() { find_in_directory(&path, "blob")? } else { path };
        let version = command_version(candidate.to_string_lossy().as_ref())?;
        return Some((candidate.to_string_lossy().into_owned(), version));
    }
    for binary in ["azurite-blob", "azurite"] {
        if command_available(binary) {
            let version = command_version(binary).unwrap_or_else(|| "installed".into());
            return Some((binary.into(), version));
        }
    }
    if command_available("npx") {
        let mut command = Command::new("npx");
        command.args(["--no-install", "azurite-blob", "--version"]);
        hide_console(&mut command);
        if let Ok(output) = command.output() {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Some(("npx --no-install azurite-blob".into(), if version.is_empty() { "installed".into() } else { version }));
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
    let mut command = Command::new(program);
    command.arg("--version");
    hide_console(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() { return None; }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn command_available(program: &str) -> bool {
    if Path::new(program).is_file() { return true; }
    let mut command = if cfg!(windows) { let mut command = Command::new("where"); command.arg(program); command } else { let mut command = Command::new("sh"); command.args(["-c", "command -v \"$1\" >/dev/null 2>&1", "aztray", program]); command };
    hide_console(&mut command);
    command.output().map(|output| output.status.success()).unwrap_or(false)
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
    match service { ServiceName::Blob => "blob", ServiceName::Queue => "queue", ServiceName::Table => "table" }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
