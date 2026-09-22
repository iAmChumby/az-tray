"use client";

import * as React from "react";
import { aztrayIpc } from "@/src/lib/ipc";
import { actionErrorMessage } from "@/src/lib/actionError";
import type { AppSnapshot, Config, EngineSnapshot, ServiceName, ServiceSnapshot, ServiceState } from "@/src/lib/types";
import type { FreePortResult } from "@/src/lib/types";

export type { Config, ServiceName, ServiceSnapshot, ServiceState };

const EMPTY_ENGINE: EngineSnapshot = {
  state: "missingAzurite",
  nodePath: null,
  azuritePath: null,
  nodeVersion: null,
  azuriteVersion: null,
  message: "Waiting for Azurite status",
  installHint: "npm install --global azurite",
};

export interface AzTrayModel {
  snapshot: AppSnapshot | null;
  services: Record<ServiceName, ServiceSnapshot> | null;
  engine: EngineSnapshot;
  loading: boolean;
  error: string | null;
  selectedService: ServiceName;
  runningCount: number;
  allRunning: boolean;
  selectService: (service: ServiceName) => void;
  lastEvent: string;
  start: (service: ServiceName) => Promise<void>;
  stop: (service: ServiceName) => Promise<void>;
  restart: (service: ServiceName) => Promise<void>;
  startAll: () => Promise<void>;
  stopAll: () => Promise<void>;
  restartAll: () => Promise<void>;
  freePort: (service: ServiceName) => Promise<FreePortResult | null>;
  saveConfig: (config: Config) => Promise<void>;
  refresh: () => Promise<void>;
  copyConnectionString: (service: ServiceName) => Promise<string>;
  saveLogs: (service?: ServiceName) => Promise<string>;
  clearLogs: (service?: ServiceName) => Promise<void>;
  quit: (mode: "stop_and_quit" | "leave_running" | "cancel") => Promise<void>;
}

const serviceTitle = (service: ServiceName) => service[0].toUpperCase() + service.slice(1);

export function useAzTray(): AzTrayModel {
  const [snapshot, setSnapshot] = React.useState<AppSnapshot | null>(null);
  const [engine, setEngine] = React.useState<EngineSnapshot>(EMPTY_ENGINE);
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState<string | null>(null);
  const [selectedService, setSelectedService] = React.useState<ServiceName>("blob");
  const [lastEvent, setLastEvent] = React.useState("Connecting to AzTray controller");

  const acceptSnapshot = React.useCallback((next: AppSnapshot) => {
    setSnapshot(next);
    setEngine(next.engine);
    setLoading(false);
    setError(null);
  }, []);

  React.useEffect(() => {
    let alive = true;
    const unlisten: (() => void)[] = [];
    void Promise.all([
      aztrayIpc.getSnapshot(),
      aztrayIpc.subscribe("snapshot_updated", (next) => {
        if (!alive) return;
        acceptSnapshot(next);
        setLastEvent("Status updated");
      }),
      aztrayIpc.subscribe("service_updated", (service) => {
        if (!alive) return;
        setSnapshot((current) => (current ? { ...current, services: { ...current.services, [service.name]: service } } : current));
        setLastEvent(`${serviceTitle(service.name)} status: ${service.state === "portInUse" ? "port in use" : service.state}`);
      }),
      aztrayIpc.subscribe("log_entry", (entry) => {
        if (!alive) return;
        setSnapshot((current) => {
          if (!current) return current;
          // Rust's empty LogStore serializes as an empty map. The first log
          // entry therefore has no pre-existing service bucket yet.
          const serviceLogs = [...(current.logs[entry.service] ?? []), entry].slice(-2000);
          return { ...current, logs: { ...current.logs, [entry.service]: serviceLogs }, mergedLogs: [...current.mergedLogs, entry].slice(-2000) };
        });
      }),
      aztrayIpc.subscribe("engine_updated", (next) => {
        if (!alive) return;
        setEngine(next);
        setSnapshot((current) => (current ? { ...current, engine: next } : current));
      }),
    ]).then(([next, ...listeners]) => {
      if (!alive) {
        listeners.forEach((listener) => typeof listener === "function" && listener());
        return;
      }
      acceptSnapshot(next as AppSnapshot);
      listeners.forEach((listener) => typeof listener === "function" && unlisten.push(listener));
      setLastEvent("Ready");
    }).catch((cause: unknown) => {
      if (!alive) return;
      setLoading(false);
      setError(actionErrorMessage(cause, "Unable to reach AzTray controller"));
      setLastEvent("Controller unavailable");
    });
    return () => {
      alive = false;
      unlisten.forEach((dispose) => dispose());
    };
  }, [acceptSnapshot]);

  const apply = React.useCallback((next: AppSnapshot) => acceptSnapshot(next), [acceptSnapshot]);
  const actionError = React.useCallback((cause: unknown, fallback: string) => {
    const message = actionErrorMessage(cause, fallback);
    setError(message);
    setLastEvent(message);
  }, []);
  const start = React.useCallback(async (service: ServiceName) => {
    try {
      apply(await aztrayIpc.startService(service));
      setLastEvent(`${serviceTitle(service)} start requested`);
    } catch (cause) { actionError(cause, `Unable to start ${serviceTitle(service)}`); }
  }, [actionError, apply]);
  const stop = React.useCallback(async (service: ServiceName) => {
    try {
      apply(await aztrayIpc.stopService(service));
      setLastEvent(`${serviceTitle(service)} stopped`);
    } catch (cause) { actionError(cause, `Unable to stop ${serviceTitle(service)}`); }
  }, [actionError, apply]);
  const restart = React.useCallback(async (service: ServiceName) => {
    try {
      apply(await aztrayIpc.restartService(service));
      setLastEvent(`${serviceTitle(service)} restarted`);
    } catch (cause) { actionError(cause, `Unable to restart ${serviceTitle(service)}`); }
  }, [actionError, apply]);
  const startAll = React.useCallback(async () => {
    try {
      apply(await aztrayIpc.startAll());
      setLastEvent("All services start requested");
    } catch (cause) { actionError(cause, "Unable to start all services"); }
  }, [actionError, apply]);
  const stopAll = React.useCallback(async () => {
    try {
      apply(await aztrayIpc.stopAll());
      setLastEvent("All app-owned services stopped");
    } catch (cause) { actionError(cause, "Unable to stop all services"); }
  }, [actionError, apply]);
  const restartAll = React.useCallback(async () => {
    try {
      apply(await aztrayIpc.restartAll());
      setLastEvent("All services restarted");
    } catch (cause) { actionError(cause, "Unable to restart all services"); }
  }, [actionError, apply]);

  const freePort = React.useCallback(async (service: ServiceName) => {
    try {
      const owner = await aztrayIpc.identifyPortOwner(service);
      if (!owner) {
        setLastEvent(`No port owner found for ${serviceTitle(service)}`);
        return null;
      }
      const result = await aztrayIpc.freePort({ serviceName: service, pid: owner.pid, startedAt: owner.startedAt });
      apply(result.snapshot);
      setLastEvent(result.message);
      return result;
    } catch (cause) {
      actionError(cause, `Unable to free ${serviceTitle(service)} port`);
      return null;
    }
  }, [actionError, apply]);

  const saveConfig = React.useCallback(async (config: Config) => {
    try {
      apply(await aztrayIpc.setConfig({ config }));
      setLastEvent("Settings saved");
    } catch (cause) { actionError(cause, "Unable to save settings"); }
  }, [actionError, apply]);
  const refresh = React.useCallback(async () => {
    try {
      apply(await aztrayIpc.getSnapshot());
      setLastEvent("Status refreshed");
    } catch (cause) { actionError(cause, "Unable to refresh status"); }
  }, [actionError, apply]);
  const copyConnectionString = React.useCallback(async (service: ServiceName) => {
    try {
      const value = await aztrayIpc.getConnectionString(service);
      if (typeof navigator !== "undefined" && navigator.clipboard) await navigator.clipboard.writeText(value);
      setLastEvent(`${serviceTitle(service)} connection string copied`);
      return value;
    } catch (cause) {
      actionError(cause, "Unable to create connection string");
      return "";
    }
  }, [actionError]);
  const saveLogs = React.useCallback(async (service?: ServiceName) => {
    try {
      let path: string | undefined;
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const chosen = await save({
          title: "Export AzTray logs",
          defaultPath: service ? `aztray-${service}-logs.txt` : "aztray-logs.txt",
          filters: [{ name: "Text log", extensions: ["txt"] }],
        });
        if (!chosen) {
          setLastEvent("Log export canceled");
          return "";
        }
        path = chosen;
      }
      const result = await aztrayIpc.saveLogs({ ...(service ? { serviceName: service } : {}), ...(path ? { path } : {}) });
      setLastEvent(`Logs saved to ${result.path}`);
      return result.path;
    } catch (cause) {
      actionError(cause, "Unable to save logs");
      return "";
    }
  }, [actionError]);
  const clearLogs = React.useCallback(async (service?: ServiceName) => {
    try {
      apply(await aztrayIpc.clearLogs(service));
      setLastEvent(service ? `${serviceTitle(service)} logs cleared` : "Logs cleared");
    } catch (cause) { actionError(cause, "Unable to clear logs"); }
  }, [actionError, apply]);
  const quit = React.useCallback(async (mode: "stop_and_quit" | "leave_running" | "cancel") => {
    if (mode === "cancel") return;
    try {
      await aztrayIpc.quitApp(mode);
      setLastEvent(mode === "stop_and_quit" ? "Stopped services and quitting" : "Leaving services running");
    } catch (cause) { actionError(cause, "Unable to quit AzTray"); }
  }, [actionError]);

  return {
    snapshot,
    services: snapshot?.services ?? null,
    engine,
    loading,
    error,
    selectedService,
    runningCount: snapshot ? Object.values(snapshot.services).filter((service) => service.state === "running").length : 0,
    allRunning: snapshot ? Object.values(snapshot.services).every((service) => service.state === "running") : false,
    selectService: setSelectedService,
    lastEvent,
    start,
    stop,
    restart,
    startAll,
    stopAll,
    restartAll,
    freePort,
    saveConfig,
    refresh,
    copyConnectionString,
    saveLogs,
    clearLogs,
    quit,
  };
}
