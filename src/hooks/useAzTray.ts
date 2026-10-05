"use client";

import * as React from "react";
import { toast } from "sonner";
import { aztrayIpc } from "@/src/lib/ipc";
import { actionErrorMessage } from "@/src/lib/actionError";
import { pickLogSavePath } from "@/src/lib/dialogs";
import { copyText } from "@/src/lib/clipboard";
import {
  DEFAULT_INSTANCE_ID,
  type AppConfig,
  type AppInfo,
  type AppSnapshot,
  type CreateInstanceRequest,
  type EngineSnapshot,
  type FreePortResult,
  type GlobalSettings,
  type InstanceSnapshot,
  type LogEntry,
  type McpStatus,
  type QuitMode,
  type ServiceName,
  type ServiceSnapshot,
  type UpdateInstanceRequest,
} from "@/src/lib/types";

const SELECTED_KEY = "aztray-instance";
const LOG_CAP = 2000;

const EMPTY_ENGINE: EngineSnapshot = {
  state: "missingAzurite",
  nodePath: null,
  azuritePath: null,
  nodeVersion: null,
  azuriteVersion: null,
  message: "Waiting for Azurite status",
  installHint: "npm install --global azurite",
};

const title = (service: ServiceName) => service[0].toUpperCase() + service.slice(1);

export interface AzTrayModel {
  snapshot: AppSnapshot | null;
  instances: InstanceSnapshot[];
  selectedInstanceId: string | null;
  selected: InstanceSnapshot | null;
  selectInstance: (id: string) => void;
  app: AppInfo | null;
  config: AppConfig | null;
  mcp: McpStatus | null;
  engine: EngineSnapshot;
  loading: boolean;
  /** Set only when the controller itself is unreachable. Action failures use toasts. */
  loadError: string | null;
  lastEvent: string;
  runningInstanceCount: number;
  /** True while an action keyed by this id (instance id, "all", "mcp", ...) is in flight. */
  isPending: (key: string) => boolean;
  startInstance: (id: string) => Promise<void>;
  stopInstance: (id: string) => Promise<void>;
  restartInstance: (id: string) => Promise<void>;
  startService: (id: string, service: ServiceName) => Promise<void>;
  stopService: (id: string, service: ServiceName) => Promise<void>;
  restartService: (id: string, service: ServiceName) => Promise<void>;
  startAll: () => Promise<void>;
  stopAll: () => Promise<void>;
  restartAll: () => Promise<void>;
  freePort: (id: string, service: ServiceName) => Promise<FreePortResult | null>;
  createInstance: (request: CreateInstanceRequest) => Promise<InstanceSnapshot | null>;
  updateInstance: (request: UpdateInstanceRequest) => Promise<InstanceSnapshot | null>;
  deleteInstance: (id: string) => Promise<boolean>;
  saveSettings: (settings: GlobalSettings) => Promise<boolean>;
  restartMcp: () => Promise<void>;
  refresh: () => Promise<void>;
  copy: (text: string, label: string) => Promise<boolean>;
  saveLogs: (instanceId?: string, service?: ServiceName) => Promise<string>;
  clearLogs: (instanceId?: string, service?: ServiceName) => Promise<void>;
  quit: (mode: QuitMode) => Promise<boolean>;
}

function readSelected(): string | null {
  try { return window.localStorage.getItem(SELECTED_KEY); } catch { return null; }
}

/** True when the entry is already present (a snapshot may have included it). Checks the tail only. */
function hasEntry(list: LogEntry[], entry: LogEntry): boolean {
  for (let i = list.length - 1; i >= 0 && i >= list.length - 50; i--) {
    if (list[i].id === entry.id) return true;
    if (list[i].sequence < entry.sequence && list[i].instanceId === entry.instanceId) break;
  }
  return false;
}

function upsert(list: InstanceSnapshot[], next: InstanceSnapshot): InstanceSnapshot[] {
  const index = list.findIndex((item) => item.config.id === next.config.id);
  if (index === -1) return [...list, next];
  const copy = list.slice();
  copy[index] = next;
  return copy;
}

export function useAzTray(): AzTrayModel {
  const [snapshot, setSnapshot] = React.useState<AppSnapshot | null>(null);
  const [loading, setLoading] = React.useState(true);
  const [loadError, setLoadError] = React.useState<string | null>(null);
  const [lastEvent, setLastEvent] = React.useState("Connecting to AzTray");
  const [selectedId, setSelectedId] = React.useState<string | null>(null);
  const [pending, setPending] = React.useState<ReadonlySet<string>>(new Set());

  React.useEffect(() => {
    setSelectedId(readSelected());
    const onStorage = (event: StorageEvent) => { if (event.key === SELECTED_KEY) setSelectedId(event.newValue); };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  const acceptSnapshot = React.useCallback((next: AppSnapshot) => {
    setSnapshot(next);
    setLoading(false);
    setLoadError(null);
  }, []);

  React.useEffect(() => {
    let alive = true;
    const disposers: (() => void)[] = [];
    const keep = (promise: Promise<() => void>) => {
      void promise.then((dispose) => { if (alive) disposers.push(dispose); else dispose(); });
    };

    keep(aztrayIpc.subscribe("snapshot_updated", (next) => { if (alive) acceptSnapshot(next); }));
    keep(aztrayIpc.subscribe("instance_updated", (instance) => {
      if (!alive) return;
      setSnapshot((current) => (current ? { ...current, instances: upsert(current.instances, instance) } : current));
    }));
    keep(aztrayIpc.subscribe("service_updated", (service: ServiceSnapshot) => {
      if (!alive) return;
      setSnapshot((current) => {
        if (!current) return current;
        const instances = current.instances.map((item) =>
          item.config.id === service.instanceId ? { ...item, services: { ...item.services, [service.name]: service } } : item,
        );
        return { ...current, instances };
      });
      setLastEvent(`${title(service.name)} on ${service.instanceId}: ${service.state === "portInUse" ? "port in use" : service.state}`);
    }));
    keep(aztrayIpc.subscribe("log_entry", (entry: LogEntry) => {
      if (!alive) return;
      setSnapshot((current) => {
        if (!current) return current;
        const instances = current.instances.map((item) => {
          if (item.config.id !== entry.instanceId) return item;
          const existing = item.logs?.[entry.service] ?? [];
          const merged = item.mergedLogs ?? [];
          const inService = hasEntry(existing, entry);
          const inMerged = hasEntry(merged, entry);
          if (inService && inMerged) return item;
          return {
            ...item,
            logs: inService ? item.logs : { ...item.logs, [entry.service]: [...existing, entry].slice(-LOG_CAP) },
            mergedLogs: inMerged ? merged : [...merged, entry].slice(-LOG_CAP),
          };
        });
        return { ...current, instances };
      });
    }));
    keep(aztrayIpc.subscribe("engine_updated", (engine) => {
      if (alive) setSnapshot((current) => (current ? { ...current, engine } : current));
    }));
    keep(aztrayIpc.subscribe("mcp_updated", (mcp) => {
      if (alive) setSnapshot((current) => (current ? { ...current, mcp } : current));
    }));

    aztrayIpc.getSnapshot().then((next) => {
      if (!alive) return;
      acceptSnapshot(next);
      setLastEvent("Ready");
    }).catch((cause: unknown) => {
      if (!alive) return;
      setLoading(false);
      setLoadError(actionErrorMessage(cause, "Unable to reach AzTray"));
      setLastEvent("AzTray is unavailable");
    });

    return () => {
      alive = false;
      disposers.forEach((dispose) => dispose());
    };
  }, [acceptSnapshot]);

  const instances = React.useMemo(() => snapshot?.instances ?? [], [snapshot]);
  const selected = React.useMemo(
    () => instances.find((item) => item.config.id === selectedId)
      ?? instances.find((item) => item.config.id === DEFAULT_INSTANCE_ID)
      ?? instances[0]
      ?? null,
    [instances, selectedId],
  );

  // If the stored selection no longer exists (deleted instance), fall back and forget it.
  React.useEffect(() => {
    if (!selectedId || instances.length === 0) return;
    if (!instances.some((item) => item.config.id === selectedId)) {
      setSelectedId(null);
      try { window.localStorage.removeItem(SELECTED_KEY); } catch { /* storage may be disabled */ }
    }
  }, [instances, selectedId]);

  const selectInstance = React.useCallback((id: string) => {
    setSelectedId(id);
    try { window.localStorage.setItem(SELECTED_KEY, id); } catch { /* storage may be disabled */ }
  }, []);

  const isPending = React.useCallback((key: string) => pending.has(key), [pending]);

  /** Runs an action with pending state and toast feedback. Returns null when it failed. */
  const run = React.useCallback(async <T,>(
    key: string,
    action: () => Promise<T>,
    messages: { ok: string | ((result: T) => string); fail: string },
  ): Promise<T | null> => {
    setPending((current) => new Set(current).add(key));
    try {
      const result = await action();
      const ok = typeof messages.ok === "function" ? messages.ok(result) : messages.ok;
      setLastEvent(ok);
      toast.success(ok);
      return result;
    } catch (cause) {
      const detail = actionErrorMessage(cause, messages.fail);
      setLastEvent(`${messages.fail}: ${detail}`);
      toast.error(messages.fail, { description: detail === messages.fail ? undefined : detail });
      return null;
    } finally {
      setPending((current) => { const next = new Set(current); next.delete(key); return next; });
    }
  }, []);

  const nameOf = React.useCallback((id: string) => snapshot?.instances.find((item) => item.config.id === id)?.config.name ?? id, [snapshot]);

  const snapshotAction = React.useCallback(
    (key: string, call: () => Promise<AppSnapshot>, ok: string, fail: string) =>
      run(key, call, { ok, fail }).then((next) => { if (next) acceptSnapshot(next); }),
    [acceptSnapshot, run],
  );

  const startInstance = React.useCallback((id: string) => snapshotAction(id, () => aztrayIpc.startInstance(id), `Started ${nameOf(id)}`, `Unable to start ${nameOf(id)}`), [nameOf, snapshotAction]);
  const stopInstance = React.useCallback((id: string) => snapshotAction(id, () => aztrayIpc.stopInstance(id), `Stopped ${nameOf(id)}`, `Unable to stop ${nameOf(id)}`), [nameOf, snapshotAction]);
  const restartInstance = React.useCallback((id: string) => snapshotAction(id, () => aztrayIpc.restartInstance(id), `Restarted ${nameOf(id)}`, `Unable to restart ${nameOf(id)}`), [nameOf, snapshotAction]);
  const startService = React.useCallback((id: string, s: ServiceName) => snapshotAction(`${id}:${s}`, () => aztrayIpc.startService(id, s), `Started ${title(s)} on ${nameOf(id)}`, `Unable to start ${title(s)}`), [nameOf, snapshotAction]);
  const stopService = React.useCallback((id: string, s: ServiceName) => snapshotAction(`${id}:${s}`, () => aztrayIpc.stopService(id, s), `Stopped ${title(s)} on ${nameOf(id)}`, `Unable to stop ${title(s)}`), [nameOf, snapshotAction]);
  const restartService = React.useCallback((id: string, s: ServiceName) => snapshotAction(`${id}:${s}`, () => aztrayIpc.restartService(id, s), `Restarted ${title(s)} on ${nameOf(id)}`, `Unable to restart ${title(s)}`), [nameOf, snapshotAction]);
  const startAll = React.useCallback(() => snapshotAction("all", aztrayIpc.startAll, "Started every instance", "Unable to start all instances"), [snapshotAction]);
  const stopAll = React.useCallback(() => snapshotAction("all", aztrayIpc.stopAll, "Stopped every instance", "Unable to stop all instances"), [snapshotAction]);
  const restartAll = React.useCallback(() => snapshotAction("all", aztrayIpc.restartAll, "Restarted every instance", "Unable to restart all instances"), [snapshotAction]);

  const freePort = React.useCallback(async (id: string, service: ServiceName) => {
    const result = await run(`${id}:${service}`, async () => {
      const owner = await aztrayIpc.identifyPortOwner(id, service);
      if (!owner) throw new Error(`Nothing is listening on the ${title(service)} port anymore.`);
      return aztrayIpc.freePort({ instanceId: id, serviceName: service, pid: owner.pid, startedAt: owner.startedAt });
    }, { ok: (r) => r.message, fail: `Unable to free the ${title(service)} port` });
    if (result) acceptSnapshot(result.snapshot);
    return result;
  }, [acceptSnapshot, run]);

  const createInstance = React.useCallback(async (request: CreateInstanceRequest) => {
    const created = await run("create", () => aztrayIpc.createInstance(request), {
      ok: (r) => (request.start ? `Created and started ${r.config.name}` : `Created ${r.config.name}`),
      fail: "Unable to create the instance",
    });
    if (created) {
      setSnapshot((current) => (current ? { ...current, instances: upsert(current.instances, created) } : current));
      selectInstance(created.config.id);
    }
    return created;
  }, [run, selectInstance]);

  const updateInstance = React.useCallback(async (request: UpdateInstanceRequest) => {
    const updated = await run(request.instanceId, () => aztrayIpc.updateInstance(request), { ok: (r) => `Saved ${r.config.name}`, fail: "Unable to save the instance" });
    if (updated) setSnapshot((current) => (current ? { ...current, instances: upsert(current.instances, updated) } : current));
    return updated;
  }, [run]);

  const deleteInstance = React.useCallback(async (id: string) => {
    const name = nameOf(id);
    const next = await run(id, () => aztrayIpc.deleteInstance(id), { ok: `Deleted ${name}. Its data folder is untouched.`, fail: `Unable to delete ${name}` });
    if (next) { acceptSnapshot(next); return true; }
    return false;
  }, [acceptSnapshot, nameOf, run]);

  const restartMcp = React.useCallback(async () => {
    const status = await run("mcp", () => aztrayIpc.restartMcp(), {
      ok: (s) => (s.running ? `MCP is running at ${s.url}` : s.enabled ? "MCP is still unavailable" : "MCP is disabled"),
      fail: "Unable to restart MCP",
    });
    if (status) setSnapshot((current) => (current ? { ...current, mcp: status } : current));
  }, [run]);

  const saveSettings = React.useCallback(async (settings: GlobalSettings) => {
    // The backend restarts MCP itself when the mcp config changed.
    const next = await run("settings", () => aztrayIpc.setSettings(settings), { ok: "Settings saved", fail: "Unable to save settings" });
    if (!next) return false;
    acceptSnapshot(next);
    return true;
  }, [acceptSnapshot, run]);

  const refresh = React.useCallback(async () => {
    try {
      acceptSnapshot(await aztrayIpc.getSnapshot());
      setLastEvent("Status refreshed");
    } catch (cause) {
      toast.error("Unable to refresh", { description: actionErrorMessage(cause, "AzTray did not respond") });
    }
  }, [acceptSnapshot]);

  const copy = React.useCallback(async (text: string, label: string) => {
    try {
      await copyText(text);
      setLastEvent(`Copied ${label}`);
      return true;
    } catch (cause) {
      toast.error(`Unable to copy ${label}`, { description: actionErrorMessage(cause, "Clipboard is unavailable") });
      return false;
    }
  }, []);

  const saveLogs = React.useCallback(async (instanceId?: string, service?: ServiceName) => {
    try {
      const base = [instanceId, service].filter(Boolean).join("-");
      const chosen = await pickLogSavePath(`aztray-${base || "all"}-logs.txt`);
      if (chosen === null) { setLastEvent("Log export canceled"); return ""; }
      const result = await aztrayIpc.saveLogs({
        ...(instanceId ? { instanceId } : {}),
        ...(service ? { serviceName: service } : {}),
        ...(chosen ? { path: chosen } : {}),
      });
      toast.success("Logs exported", { description: `${result.lineCount} lines written to ${result.path}` });
      setLastEvent(`Logs saved to ${result.path}`);
      return result.path;
    } catch (cause) {
      toast.error("Unable to export logs", { description: actionErrorMessage(cause, "The log file could not be written") });
      return "";
    }
  }, []);

  const clearLogs = React.useCallback(async (instanceId?: string, service?: ServiceName) => {
    const next = await run("logs", () => aztrayIpc.clearLogs(instanceId, service), { ok: "Logs cleared", fail: "Unable to clear logs" });
    if (next) acceptSnapshot(next);
  }, [acceptSnapshot, run]);

  const quit = React.useCallback(async (mode: QuitMode) => {
    if (mode === "cancel") return false;
    try {
      await aztrayIpc.quitApp(mode);
      return true;
    } catch (cause) {
      toast.error("Unable to quit AzTray", { description: actionErrorMessage(cause, "Quit failed") });
      return false;
    }
  }, []);

  const runningInstanceCount = instances.filter((item) => item.state === "running").length;

  return {
    snapshot,
    instances,
    selectedInstanceId: selected?.config.id ?? null,
    selected,
    selectInstance,
    app: snapshot?.app ?? null,
    config: snapshot?.config ?? null,
    mcp: snapshot?.mcp ?? null,
    engine: snapshot?.engine ?? EMPTY_ENGINE,
    loading,
    loadError,
    lastEvent,
    runningInstanceCount,
    isPending,
    startInstance,
    stopInstance,
    restartInstance,
    startService,
    stopService,
    restartService,
    startAll,
    stopAll,
    restartAll,
    freePort,
    createInstance,
    updateInstance,
    deleteInstance,
    saveSettings,
    restartMcp,
    refresh,
    copy,
    saveLogs,
    clearLogs,
    quit,
  };
}
