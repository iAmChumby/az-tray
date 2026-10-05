/**
 * Wire types. CONTRACT mirror of src-tauri/src/types.rs (docs/MULTI-INSTANCE-PLAN.md).
 * Every field name and enum value here must match the Rust serde output exactly.
 */

/** The only services AzTray manages. Wire enum values are camelCase. */
export const SERVICE_NAMES = ["blob", "queue", "table"] as const;
export type ServiceName = (typeof SERVICE_NAMES)[number];

export const SERVICE_LABELS: Record<ServiceName, string> = {
  blob: "Blob",
  queue: "Queue",
  table: "Table",
};

/** The five observable states from azctl, represented as wire-safe values. */
export type ServiceState = "stopped" | "starting" | "running" | "broken" | "portInUse";

export type LogStream = "stdout" | "stderr" | "system";
export type LogLevel = "info" | "warn" | "error";

export type EngineState =
  | "ready"
  | "missingNode"
  | "missingAzurite"
  | "invalidConfig"
  | "error";

export type ProcessIdentity = {
  pid: number;
  name: string | null;
  executablePath: string | null;
  commandLine: string | null;
  startedAt: string | null;
};

export type PortOwner = ProcessIdentity & {
  ownedByApp: boolean;
  canTerminate: boolean;
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/** Id of the instance migrated from the pre-multi-instance config. */
export const DEFAULT_INSTANCE_ID = "default";
export const CONFIG_SCHEMA_VERSION = 2;
export const DEFAULT_MCP_PORT = 47551;

/** One Azurite instance: a Blob/Queue/Table trio with its own ports and data dir. */
export type InstanceConfig = {
  /** Immutable lowercase slug [a-z0-9][a-z0-9-]{0,31}. */
  id: string;
  /** Unique (case-insensitive) display name. */
  name: string;
  host: string;
  ports: Record<ServiceName, number>;
  dataDirectory: string;
  /** Azurite --loose. */
  loose: boolean;
  /** Azurite --skipApiVersionCheck. */
  skipApiVersionCheck: boolean;
};

export type McpConfig = {
  enabled: boolean;
  port: number;
  /** Try port+1..port+9 when the preferred port is busy. */
  portFallback: boolean;
};

export const DEFAULT_MCP_CONFIG: McpConfig = { enabled: true, port: DEFAULT_MCP_PORT, portFallback: true };

/** Persisted config (schemaVersion 2). Replaces the old single-instance Config. */
export type AppConfig = {
  schemaVersion: number;
  executablePath: string | null;
  nodePath: string | null;
  mcp: McpConfig;
  instances: InstanceConfig[];
};

/** Global (non-instance) settings written by set_settings. */
export type GlobalSettings = {
  executablePath: string | null;
  nodePath: string | null;
  mcp: McpConfig;
};

export const DEFAULT_INSTANCE: InstanceConfig = {
  id: DEFAULT_INSTANCE_ID,
  name: "Default",
  host: "127.0.0.1",
  ports: { blob: 10000, queue: 10001, table: 10002 },
  dataDirectory: "",
  loose: false,
  skipApiVersionCheck: false,
};

export const DEFAULT_CONFIG: AppConfig = {
  schemaVersion: CONFIG_SCHEMA_VERSION,
  executablePath: null,
  nodePath: null,
  mcp: DEFAULT_MCP_CONFIG,
  instances: [DEFAULT_INSTANCE],
};

/** Omitted fields are auto-filled (id from name, host, next free ports, data dir). */
export type CreateInstanceRequest = {
  name: string;
  id?: string;
  host?: string;
  ports?: Record<ServiceName, number>;
  dataDirectory?: string;
  loose?: boolean;
  skipApiVersionCheck?: boolean;
  /** Start all three services after creating. */
  start?: boolean;
};

/** Only provided fields change. Everything except `name` requires a stopped instance. */
export type UpdateInstanceRequest = {
  instanceId: string;
  name?: string;
  host?: string;
  ports?: Record<ServiceName, number>;
  dataDirectory?: string;
  loose?: boolean;
  skipApiVersionCheck?: boolean;
};

/** Removes only AzTray's config entry; Azurite data on disk is never touched. */
export type DeleteInstanceRequest = { instanceId: string };

/** Pre-filled "new instance" form values. */
export type InstanceDraft = {
  name: string;
  id: string;
  host: string;
  ports: Record<ServiceName, number>;
  dataDirectory: string;
};

/** Endpoints and connection strings for one instance (always devstoreaccount1). */
export type ConnectionInfo = {
  instanceId: string;
  accountName: string;
  accountKey: string;
  /** http://host:port/devstoreaccount1 per service. */
  endpoints: Record<ServiceName, string>;
  /** Single-service connection string per service. */
  connectionStrings: Record<ServiceName, string>;
  /** One string with Blob, Queue, and Table endpoints. */
  connectionString: string;
};

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

export type EngineSnapshot = {
  state: EngineState;
  nodePath: string | null;
  azuritePath: string | null;
  nodeVersion: string | null;
  azuriteVersion: string | null;
  message: string | null;
  installHint: string | null;
};

export type ServiceSnapshot = {
  instanceId: string;
  name: ServiceName;
  state: ServiceState;
  host: string;
  port: number;
  pid: number | null;
  processIdentity: ProcessIdentity | null;
  portOwner: PortOwner | null;
  startedAt: string | null;
  stoppedAt: string | null;
  uptimeSeconds: number | null;
  exitCode: number | null;
  error: string | null;
};

export type LogEntry = {
  id: string;
  sequence: number;
  instanceId: string;
  service: ServiceName;
  stream: LogStream;
  level: LogLevel;
  message: string;
  timestamp: string;
};

/**
 * Aggregate of an instance's three services: any starting -> starting; else any
 * broken/portInUse -> broken; else all running -> running; all stopped ->
 * stopped; otherwise partial.
 */
export type InstanceState = "stopped" | "starting" | "running" | "partial" | "broken";

export type InstanceSnapshot = {
  config: InstanceConfig;
  state: InstanceState;
  services: Record<ServiceName, ServiceSnapshot>;
  connection: ConnectionInfo;
  logs: Record<ServiceName, LogEntry[]>;
  mergedLogs: LogEntry[];
};

export type McpStatus = {
  enabled: boolean;
  /** True only after bind AND a loopback self-connect succeeded. */
  running: boolean;
  /** Actual URL when running, otherwise the preferred URL. */
  url: string;
  port: number;
  requestedPort: number;
  /** port !== requestedPort. */
  fallbackUsed: boolean;
  error: string | null;
  startedAt: string | null;
  lastEvent: string | null;
  /** Bind attempts since launch. */
  attempts: number;
};

/** Build identity, so a stale installed binary is visible. */
export type AppInfo = {
  version: string;
  /** Always ["mcp", "multiInstance"] in this build. */
  features: string[];
  configPath: string;
  /** Persistent diagnostic log file. */
  logPath: string;
  /** Non-fatal config warnings (migration note, malformed file, ...). */
  configError: string | null;
};

export type AppSnapshot = {
  app: AppInfo;
  config: AppConfig;
  engine: EngineSnapshot;
  mcp: McpStatus;
  /** Config order; "default" first. */
  instances: InstanceSnapshot[];
  generatedAt: string;
};

// ---------------------------------------------------------------------------
// Command arguments / results
// ---------------------------------------------------------------------------

/**
 * Optional selector accepted by every per-instance command: an instance id or a
 * case-insensitive name. Omitted resolves to "default" if it exists, else the
 * sole instance, else an error that lists the available ids.
 */
export type InstanceSelector = { instanceId?: string };

export type LogsQuery = InstanceSelector & { serviceName?: ServiceName; limit?: number };
export type PortOwnerQuery = InstanceSelector & { serviceName: ServiceName };
export type PortOwnerExpectation = InstanceSelector & {
  serviceName: ServiceName;
  pid: number;
  startedAt: string | null;
};

export type FreePortResult = {
  snapshot: AppSnapshot;
  released: boolean;
  survivingOwner: PortOwner | null;
  message: string;
};

export type SaveLogsArgs = InstanceSelector & { serviceName?: ServiceName; path?: string };
export type SaveLogsResult = { path: string; lineCount: number };

export type QuitMode = "stop_and_quit" | "leave_running" | "cancel";
export type ServiceRef = { instanceId: string; serviceName: ServiceName };
export type QuitResult = { mode: QuitMode; stoppedServices: ServiceRef[] };

/** The typed invoke boundary. Command names intentionally remain snake_case. */
export type CommandMap = {
  // --- app / settings ---
  get_snapshot: { args: undefined; result: AppSnapshot };
  /** Replaces the old set_config. executablePath/nodePath changes need every instance stopped. */
  set_settings: { args: { settings: GlobalSettings }; result: AppSnapshot };
  check_engine: { args: undefined; result: EngineSnapshot };
  quit_app: { args: { mode: QuitMode }; result: QuitResult };
  // --- instance CRUD ---
  list_instances: { args: undefined; result: InstanceSnapshot[] };
  /** Next free ports / unique name / data dir for the "new instance" form. */
  suggest_instance: { args: { name?: string }; result: InstanceDraft };
  create_instance: { args: { request: CreateInstanceRequest }; result: InstanceSnapshot };
  update_instance: { args: { request: UpdateInstanceRequest }; result: InstanceSnapshot };
  delete_instance: { args: { request: DeleteInstanceRequest }; result: AppSnapshot };
  // --- instance lifecycle (all three services of one instance) ---
  start_instance: { args: InstanceSelector; result: AppSnapshot };
  stop_instance: { args: InstanceSelector; result: AppSnapshot };
  restart_instance: { args: InstanceSelector; result: AppSnapshot };
  // --- one service within an instance ---
  start_service: { args: InstanceSelector & { serviceName: ServiceName }; result: AppSnapshot };
  stop_service: { args: InstanceSelector & { serviceName: ServiceName }; result: AppSnapshot };
  restart_service: { args: InstanceSelector & { serviceName: ServiceName }; result: AppSnapshot };
  // --- every instance ---
  start_all: { args: undefined; result: AppSnapshot };
  stop_all: { args: undefined; result: AppSnapshot };
  restart_all: { args: undefined; result: AppSnapshot };
  // --- ports ---
  identify_port_owner: { args: PortOwnerQuery; result: PortOwner | null };
  free_port: { args: PortOwnerExpectation; result: FreePortResult };
  // --- logs ---
  get_logs: { args: LogsQuery; result: LogEntry[] };
  save_logs: { args: SaveLogsArgs; result: SaveLogsResult };
  clear_logs: { args: InstanceSelector & { serviceName?: ServiceName }; result: AppSnapshot };
  /** Last `limit` (default 500) lines of the persistent aztray.log diagnostic file. */
  get_app_log: { args: { limit?: number }; result: string[] };
  // --- connection provisioning ---
  get_connection_info: { args: InstanceSelector; result: ConnectionInfo };
  /** Without serviceName returns the combined Blob+Queue+Table string. */
  get_connection_string: { args: InstanceSelector & { serviceName?: ServiceName }; result: string };
  // --- MCP ---
  get_mcp_status: { args: undefined; result: McpStatus };
  /** Re-reads config.mcp, stops the listener, starts it again. Bind failure is reported in the status, not thrown. */
  restart_mcp: { args: undefined; result: McpStatus };
};

export type CommandName = keyof CommandMap;
export type CommandArgs<K extends CommandName> = CommandMap[K]["args"];
export type CommandResult<K extends CommandName> = CommandMap[K]["result"];

/** Tauri event names and payloads. Events are pushed after state/log changes. */
export type EventMap = {
  snapshot_updated: AppSnapshot;
  instance_updated: InstanceSnapshot;
  service_updated: ServiceSnapshot;
  log_entry: LogEntry;
  engine_updated: EngineSnapshot;
  mcp_updated: McpStatus;
  quit_requested: Record<string, never>;
};

export type EventName = keyof EventMap;
export const EVENT_NAMES: readonly EventName[] = [
  "snapshot_updated",
  "instance_updated",
  "service_updated",
  "log_entry",
  "engine_updated",
  "mcp_updated",
  "quit_requested",
];
