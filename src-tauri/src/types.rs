use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub enum ServiceName {
    Blob,
    Queue,
    Table,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ServiceState {
    Stopped,
    Starting,
    Running,
    Broken,
    PortInUse,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum LogStream {
    Stdout,
    Stderr,
    System,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum EngineState {
    Ready,
    MissingNode,
    MissingAzurite,
    InvalidConfig,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessIdentity {
    pub pid: u32,
    pub name: Option<String>,
    pub executable_path: Option<String>,
    pub command_line: Option<String>,
    pub started_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortOwner {
    #[serde(flatten)]
    pub process: ProcessIdentity,
    pub owned_by_app: bool,
    pub can_terminate: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub host: String,
    pub ports: BTreeMap<ServiceName, u16>,
    pub data_directory: String,
    pub executable_path: Option<String>,
    pub node_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSnapshot {
    pub state: EngineState,
    pub node_path: Option<String>,
    pub azurite_path: Option<String>,
    pub node_version: Option<String>,
    pub azurite_version: Option<String>,
    pub message: Option<String>,
    pub install_hint: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSnapshot {
    pub name: ServiceName,
    pub state: ServiceState,
    pub host: String,
    pub port: u16,
    pub pid: Option<u32>,
    pub process_identity: Option<ProcessIdentity>,
    pub port_owner: Option<PortOwner>,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub uptime_seconds: Option<u64>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: String,
    pub sequence: u64,
    pub service: ServiceName,
    pub stream: LogStream,
    pub level: LogLevel,
    pub message: String,
    pub timestamp: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub config: Config,
    pub engine: EngineSnapshot,
    pub services: BTreeMap<ServiceName, ServiceSnapshot>,
    pub logs: BTreeMap<ServiceName, Vec<LogEntry>>,
    pub merged_logs: Vec<LogEntry>,
    pub generated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigUpdate {
    pub config: Config,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LogsQuery {
    pub service_name: Option<ServiceName>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortOwnerQuery {
    pub service_name: ServiceName,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortOwnerExpectation {
    pub service_name: ServiceName,
    pub pid: u32,
    pub started_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreePortResult {
    pub snapshot: AppSnapshot,
    pub released: bool,
    pub surviving_owner: Option<PortOwner>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SaveLogsArgs {
    pub service_name: Option<ServiceName>,
    pub path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLogsResult {
    pub path: String,
    pub line_count: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum QuitMode {
    StopAndQuit,
    LeaveRunning,
    Cancel,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuitResult {
    pub mode: QuitMode,
    pub stopped_services: Vec<ServiceName>,
}
