/**
 * In-memory stand-in for the Tauri backend so the UI works in a plain browser
 * (`next dev`). Models multiple Azurite instances, port auto-assignment,
 * selector resolution, connection strings, logs, and every MCP state.
 *
 * Browser-only tweaks:
 *   - `?mcp=running|fallback|failed|disabled` picks the initial MCP state.
 *   - `window.__aztrayMock.setMcpMode(mode)` flips it live (also exposed in the MCP panel).
 */
import {
  CONFIG_SCHEMA_VERSION,
  DEFAULT_INSTANCE_ID,
  DEFAULT_MCP_CONFIG,
  SERVICE_NAMES,
  type AppConfig,
  type AppInfo,
  type AppSnapshot,
  type CommandArgs,
  type CommandName,
  type CommandResult,
  type ConnectionInfo,
  type CreateInstanceRequest,
  type EngineSnapshot,
  type EventMap,
  type EventName,
  type InstanceConfig,
  type InstanceDraft,
  type InstanceSnapshot,
  type InstanceState,
  type LogEntry,
  type McpStatus,
  type PortOwner,
  type ServiceName,
  type ServiceSnapshot,
  type UpdateInstanceRequest,
} from "./types";

type MockListener<K extends EventName> = (payload: EventMap[K]) => void;
const listeners = new Map<EventName, Set<(payload: unknown) => void>>();
let sequence = 0;
let ticker: ReturnType<typeof setInterval> | null = null;

const ACCOUNT_KEY = "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";
const MCP_BIND_ERROR = "bind 127.0.0.1:47551 failed: Only one usage of each socket address (protocol/network address/port) is normally permitted. (os error 10048)";

const engine: EngineSnapshot = {
  state: "ready",
  nodePath: "C:\\Program Files\\nodejs\\node.exe",
  azuritePath: "C:\\Users\\you\\AppData\\Roaming\\npm\\azurite.cmd",
  nodeVersion: "v22.11.0",
  azuriteVersion: "3.33.0",
  message: null,
  installHint: null,
};

const appInfo: AppInfo = {
  version: "0.3.0",
  features: ["mcp", "multiInstance"],
  configPath: "C:\\Users\\you\\AppData\\Roaming\\AzTray\\config.json",
  logPath: "C:\\Users\\you\\AppData\\Roaming\\AzTray\\logs\\aztray.log",
  configError: null,
};

function now() {
  return new Date().toISOString();
}

function clone<T>(value: T): T {
  return structuredClone(value);
}

function emit<K extends EventName>(event: K, payload: EventMap[K]) {
  for (const listener of listeners.get(event) ?? []) (listener as MockListener<K>)(clone(payload));
}

// --- Model -------------------------------------------------------------------

type Runtime = { services: Record<ServiceName, ServiceSnapshot>; logs: Record<ServiceName, LogEntry[]>; merged: LogEntry[] };

const seedConfig: AppConfig = {
  schemaVersion: CONFIG_SCHEMA_VERSION,
  executablePath: null,
  nodePath: null,
  mcp: { ...DEFAULT_MCP_CONFIG },
  instances: [
    { id: "default", name: "Default", host: "127.0.0.1", ports: { blob: 10000, queue: 10001, table: 10002 }, dataDirectory: "C:\\Users\\you\\.azurite", loose: false, skipApiVersionCheck: false },
    { id: "integration-tests", name: "Integration tests", host: "127.0.0.1", ports: { blob: 10003, queue: 10004, table: 10005 }, dataDirectory: "C:\\Users\\you\\AppData\\Local\\AzTray\\instances\\integration-tests\\data", loose: true, skipApiVersionCheck: false },
    { id: "legacy-api", name: "Legacy API", host: "127.0.0.1", ports: { blob: 10006, queue: 10007, table: 10008 }, dataDirectory: "D:\\work\\legacy-api\\azurite", loose: false, skipApiVersionCheck: true },
  ],
};

let config: AppConfig = clone(seedConfig);
const runtimes = new Map<string, Runtime>();

function blankService(instance: InstanceConfig, name: ServiceName): ServiceSnapshot {
  return {
    instanceId: instance.id,
    name,
    state: "stopped",
    host: instance.host,
    port: instance.ports[name],
    pid: null,
    processIdentity: null,
    portOwner: null,
    startedAt: null,
    stoppedAt: null,
    uptimeSeconds: null,
    exitCode: null,
    error: null,
  };
}

function ensureRuntime(instance: InstanceConfig): Runtime {
  let runtime = runtimes.get(instance.id);
  if (!runtime) {
    runtime = {
      services: { blob: blankService(instance, "blob"), queue: blankService(instance, "queue"), table: blankService(instance, "table") },
      logs: { blob: [], queue: [], table: [] },
      merged: [],
    };
    runtimes.set(instance.id, runtime);
  }
  return runtime;
}

function findConfig(id: string): InstanceConfig {
  const found = config.instances.find((item) => item.id === id);
  if (!found) throw new Error(`no instance matches "${id}"`);
  return found;
}

function availableList() {
  return config.instances.map((item) => `${item.id} ("${item.name}")`).join(", ");
}

/** Mirrors the engine's selector rule: id, then case-insensitive name, then default/sole. */
function resolve(selector?: string): InstanceConfig {
  if (selector) {
    const hit = config.instances.find((item) => item.id === selector) ?? config.instances.find((item) => item.name.toLowerCase() === selector.toLowerCase());
    if (!hit) throw new Error(`no instance matches "${selector}". Available: ${availableList()}`);
    return hit;
  }
  const fallback = config.instances.find((item) => item.id === DEFAULT_INSTANCE_ID) ?? (config.instances.length === 1 ? config.instances[0] : undefined);
  if (!fallback) throw new Error(`multiple instances; specify instance. Available: ${availableList()}`);
  return fallback;
}

function aggregate(services: Record<ServiceName, ServiceSnapshot>): InstanceState {
  const states = SERVICE_NAMES.map((name) => services[name].state);
  if (states.includes("starting")) return "starting";
  if (states.includes("broken") || states.includes("portInUse")) return "broken";
  if (states.every((state) => state === "running")) return "running";
  if (states.every((state) => state === "stopped")) return "stopped";
  return "partial";
}

function endpointHost(host: string) {
  return host === "0.0.0.0" || host === "::" ? "127.0.0.1" : host;
}

function connectionFor(instance: InstanceConfig): ConnectionInfo {
  const endpoints = {} as Record<ServiceName, string>;
  const connectionStrings = {} as Record<ServiceName, string>;
  const label = { blob: "Blob", queue: "Queue", table: "Table" } as const;
  for (const name of SERVICE_NAMES) {
    endpoints[name] = `http://${endpointHost(instance.host)}:${instance.ports[name]}/devstoreaccount1`;
    connectionStrings[name] = `DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=${ACCOUNT_KEY};${label[name]}Endpoint=${endpoints[name]};`;
  }
  const connectionString = `DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=${ACCOUNT_KEY};BlobEndpoint=${endpoints.blob};QueueEndpoint=${endpoints.queue};TableEndpoint=${endpoints.table};`;
  return { instanceId: instance.id, accountName: "devstoreaccount1", accountKey: ACCOUNT_KEY, endpoints, connectionStrings, connectionString };
}

function instanceSnapshot(instance: InstanceConfig): InstanceSnapshot {
  const runtime = ensureRuntime(instance);
  const services = {} as Record<ServiceName, ServiceSnapshot>;
  for (const name of SERVICE_NAMES) {
    const service = runtime.services[name];
    const uptime = service.state === "running" && service.startedAt ? Math.max(0, Math.floor((Date.now() - Date.parse(service.startedAt)) / 1000)) : null;
    services[name] = { ...service, host: instance.host, port: instance.ports[name], uptimeSeconds: uptime };
  }
  return clone({ config: instance, state: aggregate(services), services, connection: connectionFor(instance), logs: runtime.logs, mergedLogs: runtime.merged });
}

// --- MCP ---------------------------------------------------------------------

export type MockMcpMode = "running" | "fallback" | "failed" | "disabled";
let mcpMode: MockMcpMode = "running";
let mcpAttempts = 1;
let mcpStartedAt: string | null = now();

function computeMcp(): McpStatus {
  const requested = config.mcp.port;
  const effectiveMode: MockMcpMode = !config.mcp.enabled ? "disabled" : mcpMode;
  switch (effectiveMode) {
    case "running":
      return { enabled: true, running: true, url: `http://127.0.0.1:${requested}/mcp`, port: requested, requestedPort: requested, fallbackUsed: false, error: null, startedAt: mcpStartedAt, lastEvent: `Listening on 127.0.0.1:${requested}`, attempts: mcpAttempts };
    case "fallback":
      return { enabled: true, running: true, url: `http://127.0.0.1:${requested + 1}/mcp`, port: requested + 1, requestedPort: requested, fallbackUsed: true, error: null, startedAt: mcpStartedAt, lastEvent: `Port ${requested} was busy; listening on ${requested + 1} instead`, attempts: mcpAttempts };
    case "failed":
      return { enabled: true, running: false, url: `http://127.0.0.1:${requested}/mcp`, port: requested, requestedPort: requested, fallbackUsed: false, error: MCP_BIND_ERROR.replace("47551", String(requested)), startedAt: null, lastEvent: `Retrying in 15s (attempt ${mcpAttempts})`, attempts: mcpAttempts };
    default:
      return { enabled: false, running: false, url: `http://127.0.0.1:${requested}/mcp`, port: requested, requestedPort: requested, fallbackUsed: false, error: null, startedAt: null, lastEvent: "MCP is turned off in settings", attempts: 0 };
  }
}

export function getMockMcpMode(): MockMcpMode {
  return mcpMode;
}

export function setMockMcpMode(mode: MockMcpMode) {
  mcpMode = mode;
  mcpAttempts = mode === "failed" ? 3 : 1;
  mcpStartedAt = mode === "failed" || mode === "disabled" ? null : now();
  emit("mcp_updated", computeMcp());
  publish();
}

// --- Snapshots and events ----------------------------------------------------

function buildSnapshot(): AppSnapshot {
  return clone({
    app: appInfo,
    config,
    engine,
    mcp: computeMcp(),
    instances: config.instances.map(instanceSnapshot),
    generatedAt: now(),
  });
}

function publish() {
  emit("snapshot_updated", buildSnapshot());
}

function publishInstance(id: string) {
  emit("instance_updated", instanceSnapshot(findConfig(id)));
}

function addLog(instance: InstanceConfig, service: ServiceName, message: string, level: LogEntry["level"] = "info", stream: LogEntry["stream"] = "system") {
  const runtime = ensureRuntime(instance);
  const entry: LogEntry = { id: `${instance.id}-${Date.now()}-${sequence}`, sequence: sequence++, instanceId: instance.id, service, stream, level, message, timestamp: now() };
  runtime.logs[service] = [...runtime.logs[service], entry].slice(-2000);
  runtime.merged = [...runtime.merged, entry].slice(-2000);
  emit("log_entry", entry);
}

function setService(instance: InstanceConfig, name: ServiceName, patch: Partial<ServiceSnapshot>) {
  const runtime = ensureRuntime(instance);
  runtime.services[name] = { ...runtime.services[name], ...patch };
}

const pidFor = (instance: InstanceConfig, name: ServiceName) => 14000 + config.instances.indexOf(instance) * 10 + SERVICE_NAMES.indexOf(name);

function beginStart(instance: InstanceConfig, name: ServiceName) {
  setService(instance, name, { state: "starting", error: null, portOwner: null });
  addLog(instance, name, `Starting ${name} on ${instance.host}:${instance.ports[name]}`);
  window.setTimeout(() => {
    if (!config.instances.includes(instance) && !config.instances.some((item) => item.id === instance.id)) return;
    setService(instance, name, {
      state: "running",
      pid: pidFor(instance, name),
      startedAt: now(),
      processIdentity: { pid: pidFor(instance, name), name: "node.exe", executablePath: "C:\\Program Files\\nodejs\\node.exe", commandLine: `azurite-${name} --location ${instance.dataDirectory}`, startedAt: now() },
    });
    addLog(instance, name, `Azurite ${name} service is successfully listening at http://${instance.host}:${instance.ports[name]}`, "info", "stdout");
    publishInstance(instance.id);
  }, 650);
}

function haltService(instance: InstanceConfig, name: ServiceName) {
  const current = ensureRuntime(instance).services[name];
  if (current.state === "stopped") return;
  setService(instance, name, { state: "stopped", pid: null, processIdentity: null, startedAt: null, stoppedAt: now(), exitCode: 0, uptimeSeconds: null });
  addLog(instance, name, `Stopped ${name}`);
}

function startInstanceNow(instance: InstanceConfig) {
  for (const name of SERVICE_NAMES) {
    const state = ensureRuntime(instance).services[name].state;
    if (state === "stopped" || state === "broken") beginStart(instance, name);
  }
}

function stopInstanceNow(instance: InstanceConfig) {
  for (const name of SERVICE_NAMES) haltService(instance, name);
}

// --- Validation and suggestions ----------------------------------------------

function slugify(name: string) {
  const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 32).replace(/-+$/, "");
  return slug || "instance";
}

function uniqueId(base: string) {
  let candidate = base;
  let index = 2;
  while (config.instances.some((item) => item.id === candidate)) candidate = `${base.slice(0, 29)}-${index++}`;
  return candidate;
}

function takenPorts(excludeId?: string) {
  const taken = new Map<number, string>();
  for (const item of config.instances) {
    if (item.id === excludeId) continue;
    for (const name of SERVICE_NAMES) taken.set(item.ports[name], item.name);
  }
  return taken;
}

function nextPorts(): Record<ServiceName, number> {
  const taken = takenPorts();
  for (let base = 10000; base <= 19998; base += 3) {
    const trio = [base, base + 1, base + 2];
    const mcpHit = trio.some((port) => port >= config.mcp.port && port <= config.mcp.port + 9);
    if (!mcpHit && trio.every((port) => !taken.has(port))) return { blob: base, queue: base + 1, table: base + 2 };
  }
  throw new Error("no free port trio available");
}

function dataDirFor(id: string) {
  return `C:\\Users\\you\\AppData\\Local\\AzTray\\instances\\${id}\\data`;
}

function validatePorts(ports: Record<ServiceName, number>, excludeId?: string) {
  const values = SERVICE_NAMES.map((name) => ports[name]);
  if (values.some((port) => !Number.isInteger(port) || port < 1 || port > 65535)) throw new Error("ports must be between 1 and 65535");
  if (new Set(values).size !== 3) throw new Error("blob, queue, and table need three different ports");
  const taken = takenPorts(excludeId);
  for (const name of SERVICE_NAMES) {
    const owner = taken.get(ports[name]);
    if (owner) throw new Error(`${name} port ${ports[name]} is already assigned to instance "${owner}"`);
    if (ports[name] >= config.mcp.port && ports[name] <= config.mcp.port + 9) throw new Error(`${name} port ${ports[name]} is reserved for the MCP server`);
  }
}

function validateName(name: string, excludeId?: string) {
  const trimmed = name.trim();
  if (trimmed.length < 1 || trimmed.length > 48) throw new Error("name must be 1 to 48 characters");
  if (config.instances.some((item) => item.id !== excludeId && item.name.toLowerCase() === trimmed.toLowerCase())) throw new Error(`an instance named "${trimmed}" already exists`);
  return trimmed;
}

function validateDataDir(dir: string, excludeId?: string) {
  const norm = (value: string) => value.replace(/[\\/]+$/, "").toLowerCase();
  for (const item of config.instances) {
    if (item.id === excludeId) continue;
    const a = norm(item.dataDirectory);
    const b = norm(dir);
    if (a === b) throw new Error(`data directory is already used by instance "${item.name}"`);
    if (b.startsWith(`${a}\\`) || a.startsWith(`${b}\\`)) throw new Error(`data directory overlaps instance "${item.name}"`);
  }
}

function suggest(name?: string): InstanceDraft {
  const baseName = (name ?? "").trim() || "New instance";
  let finalName = baseName;
  let index = 2;
  while (config.instances.some((item) => item.name.toLowerCase() === finalName.toLowerCase())) finalName = `${baseName} ${index++}`;
  const id = uniqueId(slugify(finalName));
  return { name: finalName, id, host: "127.0.0.1", ports: nextPorts(), dataDirectory: dataDirFor(id) };
}

function createInstance(request: CreateInstanceRequest): InstanceSnapshot {
  const name = validateName(request.name);
  const draft = suggest(name);
  const id = request.id ? request.id : uniqueId(slugify(name));
  if (!/^[a-z0-9][a-z0-9-]{0,31}$/.test(id) || config.instances.some((item) => item.id === id)) throw new Error(`id "${id}" is invalid or already used`);
  const ports = request.ports ?? draft.ports;
  validatePorts(ports);
  const dataDirectory = request.dataDirectory?.trim() || dataDirFor(id);
  validateDataDir(dataDirectory);
  const instance: InstanceConfig = { id, name, host: request.host?.trim() || "127.0.0.1", ports: { ...ports }, dataDirectory, loose: request.loose ?? false, skipApiVersionCheck: request.skipApiVersionCheck ?? false };
  config = { ...config, instances: [...config.instances, instance] };
  ensureRuntime(instance);
  if (request.start) startInstanceNow(instance);
  const result = instanceSnapshot(instance);
  publish();
  return result;
}

function updateInstance(request: UpdateInstanceRequest): InstanceSnapshot {
  const instance = resolve(request.instanceId);
  const state = aggregate(ensureRuntime(instance).services);
  const structural = request.host !== undefined || request.ports !== undefined || request.dataDirectory !== undefined || request.loose !== undefined || request.skipApiVersionCheck !== undefined;
  if (structural && state !== "stopped") throw new Error(`stop "${instance.name}" before changing its host, ports, or data directory`);
  const next: InstanceConfig = { ...instance };
  if (request.name !== undefined) next.name = validateName(request.name, instance.id);
  if (request.host !== undefined) next.host = request.host.trim() || instance.host;
  if (request.ports !== undefined) { validatePorts(request.ports, instance.id); next.ports = { ...request.ports }; }
  if (request.dataDirectory !== undefined) { validateDataDir(request.dataDirectory, instance.id); next.dataDirectory = request.dataDirectory; }
  if (request.loose !== undefined) next.loose = request.loose;
  if (request.skipApiVersionCheck !== undefined) next.skipApiVersionCheck = request.skipApiVersionCheck;
  config = { ...config, instances: config.instances.map((item) => (item.id === instance.id ? next : item)) };
  const runtime = ensureRuntime(next);
  for (const name of SERVICE_NAMES) runtime.services[name] = { ...runtime.services[name], host: next.host, port: next.ports[name] };
  const result = instanceSnapshot(next);
  publish();
  return result;
}

// --- Seeding -----------------------------------------------------------------

function seed() {
  if (runtimes.size) return;
  for (const instance of config.instances) ensureRuntime(instance);
  const main = config.instances[0];
  for (const name of SERVICE_NAMES) {
    setService(main, name, {
      state: "running",
      pid: pidFor(main, name),
      startedAt: new Date(Date.now() - 1000 * 60 * 47).toISOString(),
      processIdentity: { pid: pidFor(main, name), name: "node.exe", executablePath: "C:\\Program Files\\nodejs\\node.exe", commandLine: `azurite-${name} --location ${main.dataDirectory}`, startedAt: null },
    });
  }
  const lines: [ServiceName, string, LogEntry["level"], LogEntry["stream"]][] = [
    ["blob", "Azurite Blob service is starting on 127.0.0.1:10000", "info", "stdout"],
    ["blob", "Azurite Blob service is successfully listening at http://127.0.0.1:10000", "info", "stdout"],
    ["queue", "Azurite Queue service is successfully listening at http://127.0.0.1:10001", "info", "stdout"],
    ["table", "Azurite Table service is successfully listening at http://127.0.0.1:10002", "info", "stdout"],
    ["blob", "127.0.0.1 - - [PUT] /devstoreaccount1/uploads?restype=container 201", "info", "stdout"],
    ["table", "Loose mode is off: unsupported headers will be rejected", "warn", "stderr"],
    ["queue", "127.0.0.1 - - [POST] /devstoreaccount1/jobs/messages 201", "info", "stdout"],
  ];
  lines.forEach(([service, message, level, stream], index) => {
    addLog(main, service, message, level, stream);
    const entry = ensureRuntime(main).merged.at(-1);
    if (entry) entry.timestamp = new Date(Date.now() - (lines.length - index) * 41_000).toISOString();
  });
  const legacy = config.instances[2];
  const owner: PortOwner = { pid: 4416, name: "node.exe", executablePath: "C:\\Program Files\\nodejs\\node.exe", commandLine: "node azurite-table --tablePort 10008", startedAt: new Date(Date.now() - 3_600_000).toISOString(), ownedByApp: false, canTerminate: true };
  setService(legacy, "table", { state: "portInUse", portOwner: owner, error: "Port 10008 is already in use by node.exe (PID 4416)." });
  addLog(legacy, "table", "Cannot start table: port 10008 is held by node.exe (PID 4416)", "error");
  const modeParam = typeof window !== "undefined" ? new URLSearchParams(window.location.search).get("mcp") : null;
  if (modeParam === "running" || modeParam === "fallback" || modeParam === "failed" || modeParam === "disabled") {
    mcpMode = modeParam;
    mcpAttempts = modeParam === "failed" ? 3 : 1;
    if (modeParam === "failed" || modeParam === "disabled") mcpStartedAt = null;
  }
}

function startTicker() {
  if (ticker || typeof window === "undefined") return;
  ticker = setInterval(() => {
    for (const instance of config.instances) {
      const running = SERVICE_NAMES.filter((name) => ensureRuntime(instance).services[name].state === "running");
      if (!running.length) continue;
      const name = running[Math.floor(Math.random() * running.length)];
      const verb = name === "blob" ? "[GET] /devstoreaccount1/uploads/report.csv 200" : name === "queue" ? "[GET] /devstoreaccount1/jobs/messages 200" : "[POST] /devstoreaccount1/Tables 201";
      addLog(instance, name, `127.0.0.1 - - ${verb}`, "info", "stdout");
    }
  }, 6000);
}

export function isMockRuntime() {
  return typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
}

if (typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window)) {
  (window as unknown as { __aztrayMock: unknown }).__aztrayMock = { setMcpMode: setMockMcpMode, getMcpMode: getMockMcpMode };
}

// --- Command dispatch --------------------------------------------------------

const APP_LOG_LINES = [
  "2026-10-05T09:12:03Z INFO  AzTray 0.3.0 starting (config: C:\\Users\\you\\AppData\\Roaming\\AzTray\\config.json)",
  "2026-10-05T09:12:03Z INFO  mcp: attempt 1: binding 127.0.0.1:47551",
  "2026-10-05T09:12:03Z INFO  mcp: running at http://127.0.0.1:47551/mcp",
  "2026-10-05T09:12:04Z INFO  instance default: starting blob on 127.0.0.1:10000",
  "2026-10-05T09:12:05Z WARN  instance legacy-api: table port 10008 held by node.exe (PID 4416)",
];

export async function mockInvoke<K extends CommandName>(command: K, args: CommandArgs<K>): Promise<CommandResult<K>> {
  seed();
  startTicker();
  const a = (args ?? {}) as Record<string, unknown>;
  const ret = <T,>(value: T) => clone(value) as unknown as CommandResult<K>;
  const instanceArg = () => resolve(a.instanceId as string | undefined);

  switch (command) {
    case "get_snapshot":
      return ret(buildSnapshot());
    case "set_settings": {
      const settings = a.settings as { executablePath: string | null; nodePath: string | null; mcp: AppConfig["mcp"] };
      if (settings.mcp.port < 1024 || settings.mcp.port > 65535) throw new Error("MCP port must be between 1024 and 65535");
      for (const item of config.instances) for (const name of SERVICE_NAMES) {
        if (item.ports[name] >= settings.mcp.port && item.ports[name] <= settings.mcp.port + 9) throw new Error(`MCP port range overlaps ${name} port ${item.ports[name]} of "${item.name}"`);
      }
      const mcpChanged = JSON.stringify(config.mcp) !== JSON.stringify(settings.mcp);
      config = { ...config, executablePath: settings.executablePath, nodePath: settings.nodePath, mcp: { ...settings.mcp } };
      if (mcpChanged) {
        // Mirrors the backend: a changed mcp config restarts the MCP server.
        if (config.mcp.enabled) {
          mcpAttempts += 1;
          if (mcpMode === "failed" && config.mcp.portFallback) mcpMode = "fallback";
          mcpStartedAt = mcpMode === "failed" ? null : now();
        }
        emit("mcp_updated", computeMcp());
      }
      publish();
      return ret(buildSnapshot());
    }
    case "check_engine":
      return ret(engine);
    case "quit_app":
      return ret({ mode: a.mode, stoppedServices: [] });
    case "list_instances":
      return ret(config.instances.map(instanceSnapshot));
    case "suggest_instance":
      return ret(suggest(a.name as string | undefined));
    case "create_instance":
      return ret(createInstance(a.request as CreateInstanceRequest));
    case "update_instance":
      return ret(updateInstance(a.request as UpdateInstanceRequest));
    case "delete_instance": {
      const instance = resolve((a.request as { instanceId: string }).instanceId);
      if (config.instances.length === 1) throw new Error("the last instance cannot be deleted");
      if (aggregate(ensureRuntime(instance).services) !== "stopped") throw new Error(`stop "${instance.name}" before deleting it`);
      config = { ...config, instances: config.instances.filter((item) => item.id !== instance.id) };
      runtimes.delete(instance.id);
      publish();
      return ret(buildSnapshot());
    }
    case "start_instance": {
      const instance = instanceArg();
      startInstanceNow(instance);
      publish();
      return ret(buildSnapshot());
    }
    case "stop_instance": {
      stopInstanceNow(instanceArg());
      publish();
      return ret(buildSnapshot());
    }
    case "restart_instance": {
      const instance = instanceArg();
      stopInstanceNow(instance);
      startInstanceNow(instance);
      publish();
      return ret(buildSnapshot());
    }
    case "start_service": {
      beginStart(instanceArg(), a.serviceName as ServiceName);
      publish();
      return ret(buildSnapshot());
    }
    case "stop_service": {
      haltService(instanceArg(), a.serviceName as ServiceName);
      publish();
      return ret(buildSnapshot());
    }
    case "restart_service": {
      const instance = instanceArg();
      haltService(instance, a.serviceName as ServiceName);
      beginStart(instance, a.serviceName as ServiceName);
      publish();
      return ret(buildSnapshot());
    }
    case "start_all":
      config.instances.forEach(startInstanceNow);
      publish();
      return ret(buildSnapshot());
    case "stop_all":
      config.instances.forEach(stopInstanceNow);
      publish();
      return ret(buildSnapshot());
    case "restart_all":
      config.instances.forEach((item) => { stopInstanceNow(item); startInstanceNow(item); });
      publish();
      return ret(buildSnapshot());
    case "identify_port_owner": {
      const instance = instanceArg();
      return ret(ensureRuntime(instance).services[a.serviceName as ServiceName].portOwner);
    }
    case "free_port": {
      const instance = instanceArg();
      const name = a.serviceName as ServiceName;
      setService(instance, name, { state: "stopped", portOwner: null, error: null });
      addLog(instance, name, `Released port ${instance.ports[name]}`);
      publish();
      return ret({ snapshot: buildSnapshot(), released: true, survivingOwner: null, message: `Freed port ${instance.ports[name]}` });
    }
    case "get_logs": {
      const instance = instanceArg();
      const runtime = ensureRuntime(instance);
      const query = a as { serviceName?: ServiceName; limit?: number };
      const source = query.serviceName ? runtime.logs[query.serviceName] : runtime.merged;
      return ret(source.slice(-(query.limit ?? 500)));
    }
    case "save_logs": {
      const instance = instanceArg();
      return ret({ path: String(a.path ?? `C:\\Users\\you\\Documents\\aztray-${instance.id}-logs.txt`), lineCount: ensureRuntime(instance).merged.length });
    }
    case "clear_logs": {
      const instance = instanceArg();
      const runtime = ensureRuntime(instance);
      const name = a.serviceName as ServiceName | undefined;
      if (name) {
        runtime.logs[name] = [];
        runtime.merged = runtime.merged.filter((entry) => entry.service !== name);
      } else {
        runtime.logs = { blob: [], queue: [], table: [] };
        runtime.merged = [];
      }
      publish();
      return ret(buildSnapshot());
    }
    case "get_app_log": {
      const limit = (a.limit as number | undefined) ?? 500;
      const extra = computeMcp().running ? [] : [`${now()} ERROR mcp: ${computeMcp().error ?? "bind failed"}`];
      return ret([...APP_LOG_LINES, ...extra].slice(-limit));
    }
    case "get_connection_info":
      return ret(connectionFor(instanceArg()));
    case "get_connection_string": {
      const info = connectionFor(instanceArg());
      return ret(a.serviceName ? info.connectionStrings[a.serviceName as ServiceName] : info.connectionString);
    }
    case "get_mcp_status":
      return ret(computeMcp());
    case "restart_mcp": {
      if (config.mcp.enabled) {
        mcpAttempts += 1;
        if (mcpMode === "failed" && config.mcp.portFallback) mcpMode = "fallback";
        mcpStartedAt = mcpMode === "failed" ? null : now();
      }
      const status = computeMcp();
      emit("mcp_updated", status);
      publish();
      return ret(status);
    }
    default:
      throw new Error(`Mock command not implemented: ${String(command)}`);
  }
}

export function subscribeMock<K extends EventName>(event: K, listener: MockListener<K>) {
  const set = listeners.get(event) ?? new Set<(payload: unknown) => void>();
  set.add(listener as (payload: unknown) => void);
  listeners.set(event, set);
  return () => { set.delete(listener as (payload: unknown) => void); };
}
