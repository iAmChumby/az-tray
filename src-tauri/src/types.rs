//! Shared wire types. This file is the CONTRACT for the multi-instance work:
//! see docs/MULTI-INSTANCE-PLAN.md. All structs serialize camelCase; enums
//! camelCase (QuitMode is snake_case, as before). TS mirror: src/lib/types.ts.

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

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Id of the instance created from the pre-multi-instance config. It is
/// always the first instance and the target of any call that omits an
/// instance selector while it exists.
pub const DEFAULT_INSTANCE_ID: &str = "default";
pub const CONFIG_SCHEMA_VERSION: u32 = 2;
pub const DEFAULT_MCP_PORT: u16 = 47_551;

fn default_true() -> bool {
    true
}

fn default_mcp_port() -> u16 {
    DEFAULT_MCP_PORT
}

/// One independently configured Azurite instance (a Blob/Queue/Table trio with
/// its own ports and data directory). `id` is a lowercase slug
/// (`[a-z0-9][a-z0-9-]{0,31}`), unique and immutable. `name` is a unique
/// (case-insensitive) display name.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstanceConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub ports: BTreeMap<ServiceName, u16>,
    pub data_directory: String,
    /// Passes `--loose` to Azurite.
    #[serde(default)]
    pub loose: bool,
    /// Passes `--skipApiVersionCheck` to Azurite.
    #[serde(default)]
    pub skip_api_version_check: bool,
}

/// Settings for AzTray's local MCP endpoint. Defaults: enabled, port 47551,
/// fall back to the next free port (up to +9) when the preferred port is busy.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_mcp_port")]
    pub port: u16,
    #[serde(default = "default_true")]
    pub port_fallback: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: DEFAULT_MCP_PORT,
            port_fallback: true,
        }
    }
}

/// Persisted AzTray configuration (`%APPDATA%\AzTray\config.json`,
/// `schemaVersion` 2). Replaces the old single-instance `Config`. Engine
/// settings (`executablePath`, `nodePath`) are global to all instances.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub schema_version: u32,
    pub executable_path: Option<String>,
    pub node_path: Option<String>,
    pub mcp: McpConfig,
    pub instances: Vec<InstanceConfig>,
}

/// Global (non-instance) settings written by `set_settings`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSettings {
    pub executable_path: Option<String>,
    pub node_path: Option<String>,
    pub mcp: McpConfig,
}

/// Body of `create_instance`. Omitted fields are auto-filled: `id` a slug of
/// `name`, `host` 127.0.0.1, `ports` the next free trio, `dataDirectory`
/// `<LOCALAPPDATA>\AzTray\instances\<id>`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstanceRequest {
    pub name: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub ports: Option<BTreeMap<ServiceName, u16>>,
    #[serde(default)]
    pub data_directory: Option<String>,
    #[serde(default)]
    pub loose: bool,
    #[serde(default)]
    pub skip_api_version_check: bool,
    /// Start all three services right after creating the instance.
    #[serde(default)]
    pub start: bool,
}

/// Body of `update_instance`. Only provided fields change. Changing host,
/// ports, dataDirectory, loose, or skipApiVersionCheck requires the instance
/// to be fully stopped; `name` can change at any time.
#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstanceRequest {
    pub instance_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub ports: Option<BTreeMap<ServiceName, u16>>,
    #[serde(default)]
    pub data_directory: Option<String>,
    #[serde(default)]
    pub loose: Option<bool>,
    #[serde(default)]
    pub skip_api_version_check: Option<bool>,
}

/// Body of `delete_instance`. Deleting removes only the instance's AzTray
/// config entry and in-memory logs; stored Azurite data is never touched. The
/// instance must be stopped, and the last remaining instance cannot be deleted.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteInstanceRequest {
    pub instance_id: String,
}

/// Pre-filled values for the "new instance" form (`suggest_instance`).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceDraft {
    pub name: String,
    pub id: String,
    pub host: String,
    pub ports: BTreeMap<ServiceName, u16>,
    pub data_directory: String,
}

/// Endpoints and connection strings for one instance. The account is always
/// Azurite's well-known `devstoreaccount1` development account; the key is a
/// public emulator constant. A bind host of 0.0.0.0 or :: is reported as
/// 127.0.0.1.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub instance_id: String,
    pub account_name: String,
    pub account_key: String,
    /// `http://host:port/devstoreaccount1` per service.
    pub endpoints: BTreeMap<ServiceName, String>,
    /// Single-service connection string per service.
    pub connection_strings: BTreeMap<ServiceName, String>,
    /// One connection string carrying Blob, Queue, and Table endpoints.
    pub connection_string: String,
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

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
    pub instance_id: String,
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
    pub instance_id: String,
    pub service: ServiceName,
    pub stream: LogStream,
    pub level: LogLevel,
    pub message: String,
    pub timestamp: String,
}

/// Aggregate state of an instance, derived from its three services:
/// any starting -> Starting; else any broken/portInUse -> Broken; else all
/// running -> Running; else all stopped -> Stopped; else Partial.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum InstanceState {
    Stopped,
    Starting,
    Running,
    Partial,
    Broken,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSnapshot {
    pub config: InstanceConfig,
    pub state: InstanceState,
    pub services: BTreeMap<ServiceName, ServiceSnapshot>,
    pub connection: ConnectionInfo,
    /// Bounded per-service logs for this instance.
    pub logs: BTreeMap<ServiceName, Vec<LogEntry>>,
    /// Merged arrival-order logs for this instance.
    pub merged_logs: Vec<LogEntry>,
}

/// Live status of the local MCP endpoint. `enabled` mirrors config. `running`
/// is true only after the socket bound AND a loopback self-connect succeeded.
/// `url` is the actual URL when running, otherwise the preferred URL.
/// `fallbackUsed` is true when `port` differs from `requestedPort`.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub enabled: bool,
    pub running: bool,
    pub url: String,
    pub port: u16,
    pub requested_port: u16,
    pub fallback_used: bool,
    pub error: Option<String>,
    pub started_at: Option<String>,
    /// Human-readable last lifecycle event ("Listening on 127.0.0.1:47551").
    pub last_event: Option<String>,
    /// Bind attempts since AzTray started (supervisor retries increment it).
    pub attempts: u32,
}

impl McpStatus {
    /// Status before the first start attempt.
    pub fn initial(config: &McpConfig) -> Self {
        Self {
            enabled: config.enabled,
            running: false,
            url: format!("http://127.0.0.1:{}/mcp", config.port),
            port: config.port,
            requested_port: config.port,
            fallback_used: false,
            error: None,
            started_at: None,
            last_event: None,
            attempts: 0,
        }
    }
}

/// Build/runtime identity so a stale installed binary is visible in the UI.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// `CARGO_PKG_VERSION`.
    pub version: String,
    /// Compiled capabilities, always ["mcp", "multiInstance"] in this build.
    pub features: Vec<String>,
    pub config_path: String,
    /// Persistent diagnostic log (`%APPDATA%\AzTray\logs\aztray.log`).
    pub log_path: String,
    /// Non-fatal config warnings (migration notes, malformed file, ...).
    pub config_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub app: AppInfo,
    pub config: AppConfig,
    pub engine: EngineSnapshot,
    pub mcp: McpStatus,
    /// Instances in config order; `default` first.
    pub instances: Vec<InstanceSnapshot>,
    pub generated_at: String,
}

// ---------------------------------------------------------------------------
// Command arguments / results
// ---------------------------------------------------------------------------

/// `instance_id: None` resolves with the rule in the plan doc: the `default`
/// instance if present, else the sole instance, else an error listing ids.
/// Selectors accept an instance id or a case-insensitive name.
#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LogsQuery {
    #[serde(default)]
    pub instance_id: Option<String>,
    pub service_name: Option<ServiceName>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortOwnerQuery {
    #[serde(default)]
    pub instance_id: Option<String>,
    pub service_name: ServiceName,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortOwnerExpectation {
    #[serde(default)]
    pub instance_id: Option<String>,
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
    #[serde(default)]
    pub instance_id: Option<String>,
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

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRef {
    pub instance_id: String,
    pub service_name: ServiceName,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuitResult {
    pub mode: QuitMode,
    pub stopped_services: Vec<ServiceRef>,
}
