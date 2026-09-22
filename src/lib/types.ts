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

export type Config = {
  host: string;
  ports: Record<ServiceName, number>;
  dataDirectory: string;
  executablePath: string | null;
  nodePath: string | null;
};

export const DEFAULT_CONFIG: Config = {
  host: "127.0.0.1",
  ports: { blob: 10000, queue: 10001, table: 10002 },
  dataDirectory: "",
  executablePath: null,
  nodePath: null,
};

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
  service: ServiceName;
  stream: LogStream;
  level: LogLevel;
  message: string;
  timestamp: string;
};

export type AppSnapshot = {
  config: Config;
  engine: EngineSnapshot;
  services: Record<ServiceName, ServiceSnapshot>;
  logs: Record<ServiceName, LogEntry[]>;
  mergedLogs: LogEntry[];
  generatedAt: string;
};

export type ConfigUpdate = { config: Config };
export type LogsQuery = { serviceName?: ServiceName; limit?: number };
export type PortOwnerQuery = { serviceName: ServiceName };
export type PortOwnerExpectation = {
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

export type SaveLogsArgs = { serviceName?: ServiceName; path?: string };
export type SaveLogsResult = { path: string; lineCount: number };

export type QuitMode = "stop_and_quit" | "leave_running" | "cancel";
export type QuitResult = { mode: QuitMode; stoppedServices: ServiceName[] };

/** The typed invoke boundary. Command names intentionally remain snake_case. */
export type CommandMap = {
  get_snapshot: { args: undefined; result: AppSnapshot };
  set_config: { args: ConfigUpdate; result: AppSnapshot };
  check_engine: { args: undefined; result: EngineSnapshot };
  start_service: { args: { serviceName: ServiceName }; result: AppSnapshot };
  stop_service: { args: { serviceName: ServiceName }; result: AppSnapshot };
  restart_service: { args: { serviceName: ServiceName }; result: AppSnapshot };
  start_all: { args: undefined; result: AppSnapshot };
  stop_all: { args: undefined; result: AppSnapshot };
  restart_all: { args: undefined; result: AppSnapshot };
  identify_port_owner: { args: PortOwnerQuery; result: PortOwner | null };
  free_port: { args: PortOwnerExpectation; result: FreePortResult };
  get_logs: { args: LogsQuery; result: LogEntry[] };
  save_logs: { args: SaveLogsArgs; result: SaveLogsResult };
  get_connection_string: { args: { serviceName: ServiceName }; result: string };
  clear_logs: { args: { serviceName?: ServiceName }; result: AppSnapshot };
  quit_app: { args: { mode: QuitMode }; result: QuitResult };
};

export type CommandName = keyof CommandMap;
export type CommandArgs<K extends CommandName> = CommandMap[K]["args"];
export type CommandResult<K extends CommandName> = CommandMap[K]["result"];

/** Tauri event names and payloads. Events are pushed after state/log changes. */
export type EventMap = {
  snapshot_updated: AppSnapshot;
  service_updated: ServiceSnapshot;
  log_entry: LogEntry;
  engine_updated: EngineSnapshot;
  quit_requested: Record<string, never>;
};

export type EventName = keyof EventMap;
export const EVENT_NAMES: readonly EventName[] = [
  "snapshot_updated",
  "service_updated",
  "log_entry",
  "engine_updated",
  "quit_requested",
];
