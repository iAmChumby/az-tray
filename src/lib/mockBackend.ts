import {
  DEFAULT_CONFIG,
  type AppSnapshot,
  type CommandArgs,
  type CommandName,
  type CommandResult,
  type EngineSnapshot,
  type EventMap,
  type EventName,
  type LogEntry,
  type PortOwner,
  type ServiceName,
  type ServiceSnapshot,
} from "./types";

type MockListener<K extends EventName> = (payload: EventMap[K]) => void;
const listeners = new Map<EventName, Set<(payload: unknown) => void>>();
let sequence = 0;

const engine: EngineSnapshot = {
  state: "missingAzurite",
  nodePath: null,
  azuritePath: null,
  nodeVersion: null,
  azuriteVersion: null,
  message: "Browser mock mode is active. Configure Azurite in the native app.",
  installHint: "npm install --global azurite",
};

function now() {
  return new Date().toISOString();
}

function serviceSnapshot(name: ServiceName): ServiceSnapshot {
  return {
    name,
    state: "stopped",
    host: DEFAULT_CONFIG.host,
    port: DEFAULT_CONFIG.ports[name],
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

let snapshot: AppSnapshot = {
  config: structuredClone(DEFAULT_CONFIG),
  engine,
  services: { blob: serviceSnapshot("blob"), queue: serviceSnapshot("queue"), table: serviceSnapshot("table") },
  logs: { blob: [], queue: [], table: [] },
  mergedLogs: [],
  generatedAt: now(),
};

function clone<T>(value: T): T {
  return structuredClone(value);
}

function emit<K extends EventName>(event: K, payload: EventMap[K]) {
  for (const listener of listeners.get(event) ?? []) (listener as MockListener<K>)(clone(payload));
}

function update(next: AppSnapshot) {
  snapshot = { ...next, generatedAt: now() };
  emit("snapshot_updated", snapshot);
}

function addLog(service: ServiceName, message: string, level: LogEntry["level"] = "info") {
  const entry: LogEntry = {
    id: `${Date.now()}-${sequence}`,
    sequence: sequence++,
    service,
    stream: "system",
    level,
    message,
    timestamp: now(),
  };
  const serviceLogs = [...snapshot.logs[service], entry].slice(-2000);
  update({ ...snapshot, logs: { ...snapshot.logs, [service]: serviceLogs }, mergedLogs: [...snapshot.mergedLogs, entry].slice(-2000) });
  emit("log_entry", entry);
}

function setState(name: ServiceName, state: ServiceSnapshot["state"]) {
  const current = snapshot.services[name];
  const next: ServiceSnapshot = {
    ...current,
    state,
    pid: state === "running" ? 10000 + sequence : null,
    startedAt: state === "running" ? now() : current.startedAt,
    stoppedAt: state === "stopped" ? now() : current.stoppedAt,
    error: state === "broken" ? "Mock service failure" : null,
  };
  update({ ...snapshot, services: { ...snapshot.services, [name]: next } });
  emit("service_updated", next);
}

export function isMockRuntime() {
  return typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
}

export async function mockInvoke<K extends CommandName>(command: K, args: CommandArgs<K>): Promise<CommandResult<K>> {
  switch (command) {
    case "get_snapshot":
      return clone(snapshot) as CommandResult<K>;
    case "set_config": {
      const config = (args as { config: typeof DEFAULT_CONFIG }).config;
      update({ ...snapshot, config: clone(config) });
      return clone(snapshot) as CommandResult<K>;
    }
    case "check_engine":
      return clone(engine) as CommandResult<K>;
    case "start_service": {
      const serviceName = (args as { serviceName: ServiceName }).serviceName;
      setState(serviceName, "starting");
      setState(serviceName, "running");
      addLog(serviceName, `Mock ${serviceName} service started`);
      return clone(snapshot) as CommandResult<K>;
    }
    case "stop_service": {
      const serviceName = (args as { serviceName: ServiceName }).serviceName;
      setState(serviceName, "stopped");
      addLog(serviceName, `Mock ${serviceName} service stopped`);
      return clone(snapshot) as CommandResult<K>;
    }
    case "restart_service": {
      const serviceName = (args as { serviceName: ServiceName }).serviceName;
      setState(serviceName, "stopped");
      setState(serviceName, "starting");
      setState(serviceName, "running");
      addLog(serviceName, `Mock ${serviceName} service restarted`);
      return clone(snapshot) as CommandResult<K>;
    }
    case "start_all":
      for (const name of ["blob", "queue", "table"] as ServiceName[]) {
        setState(name, "starting");
        setState(name, "running");
      }
      return clone(snapshot) as CommandResult<K>;
    case "stop_all":
      for (const name of ["blob", "queue", "table"] as ServiceName[]) setState(name, "stopped");
      return clone(snapshot) as CommandResult<K>;
    case "restart_all":
      for (const name of ["blob", "queue", "table"] as ServiceName[]) {
        setState(name, "stopped");
        setState(name, "starting");
        setState(name, "running");
      }
      return clone(snapshot) as CommandResult<K>;
    case "identify_port_owner": {
      const serviceName = (args as { serviceName: ServiceName }).serviceName;
      return clone(snapshot.services[serviceName].portOwner) as CommandResult<K>;
    }
    case "free_port":
      return { snapshot: clone(snapshot), released: true, survivingOwner: null, message: "Mock port released." } as CommandResult<K>;
    case "get_logs": {
      const query = args as { serviceName?: ServiceName; limit?: number };
      const logs = query.serviceName ? snapshot.logs[query.serviceName] : snapshot.mergedLogs;
      return clone(logs.slice(-(query.limit ?? 200))) as CommandResult<K>;
    }
    case "save_logs":
      return { path: String((args as { path?: string }).path ?? "aztray.log"), lineCount: snapshot.mergedLogs.length } as CommandResult<K>;
    case "get_connection_string": {
      const serviceName = (args as { serviceName: ServiceName }).serviceName;
      const port = snapshot.config.ports[serviceName];
      const endpoint = `http://${snapshot.config.host}:${port}`;
      return `DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1Jnh${serviceName};${serviceName[0].toUpperCase()}${serviceName.slice(1)}Endpoint=${endpoint};` as CommandResult<K>;
    }
    case "clear_logs": {
      const serviceName = (args as { serviceName?: ServiceName }).serviceName;
      update({
        ...snapshot,
        logs: serviceName ? { ...snapshot.logs, [serviceName]: [] } : { blob: [], queue: [], table: [] },
        mergedLogs: serviceName ? snapshot.mergedLogs.filter((entry) => entry.service !== serviceName) : [],
      });
      return clone(snapshot) as CommandResult<K>;
    }
    case "quit_app":
      return { mode: (args as { mode: "stop_and_quit" | "leave_running" | "cancel" }).mode, stoppedServices: [] } as CommandResult<K>;
    default:
      throw new Error(`Mock command not implemented: ${String(command)}`);
  }
}

export function subscribeMock<K extends EventName>(event: K, listener: MockListener<K>) {
  const set = listeners.get(event) ?? new Set<(payload: unknown) => void>();
  set.add(listener as (payload: unknown) => void);
  listeners.set(event, set);
  return () => set.delete(listener as (payload: unknown) => void);
}
