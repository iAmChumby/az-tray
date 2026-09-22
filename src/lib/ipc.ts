import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isMockRuntime, mockInvoke, subscribeMock } from "./mockBackend";
import type {
  CommandArgs,
  CommandMap,
  CommandName,
  CommandResult,
  EventMap,
  EventName,
} from "./types";

function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function invokeCommand<K extends CommandName>(command: K, args: CommandArgs<K>): Promise<CommandResult<K>> {
  if (isMockRuntime() || !isTauriRuntime()) return mockInvoke(command, args);
  return invoke<CommandResult<K>>(command, args as Record<string, unknown> | undefined);
}

export function getSnapshot() {
  return invokeCommand("get_snapshot", undefined);
}

export function setConfig(args: CommandMap["set_config"]["args"]) {
  return invokeCommand("set_config", args);
}

export function checkEngine() {
  return invokeCommand("check_engine", undefined);
}

export function startService(serviceName: CommandMap["start_service"]["args"]["serviceName"]) {
  return invokeCommand("start_service", { serviceName });
}

export function stopService(serviceName: CommandMap["stop_service"]["args"]["serviceName"]) {
  return invokeCommand("stop_service", { serviceName });
}

export function restartService(serviceName: CommandMap["restart_service"]["args"]["serviceName"]) {
  return invokeCommand("restart_service", { serviceName });
}

export function startAll() {
  return invokeCommand("start_all", undefined);
}

export function stopAll() {
  return invokeCommand("stop_all", undefined);
}

export function restartAll() {
  return invokeCommand("restart_all", undefined);
}

export function identifyPortOwner(serviceName: CommandMap["identify_port_owner"]["args"]["serviceName"]) {
  return invokeCommand("identify_port_owner", { serviceName });
}

export function freePort(args: CommandMap["free_port"]["args"]) {
  return invokeCommand("free_port", args);
}

export function getLogs(args: CommandMap["get_logs"]["args"] = {}) {
  return invokeCommand("get_logs", args);
}

export function saveLogs(args: CommandMap["save_logs"]["args"] = {}) {
  return invokeCommand("save_logs", args);
}

export function getConnectionString(serviceName: CommandMap["get_connection_string"]["args"]["serviceName"]) {
  return invokeCommand("get_connection_string", { serviceName });
}

export function clearLogs(serviceName?: CommandMap["clear_logs"]["args"]["serviceName"]) {
  return invokeCommand("clear_logs", serviceName ? { serviceName } : {});
}

export function quitApp(mode: CommandMap["quit_app"]["args"]["mode"]) {
  return invokeCommand("quit_app", { mode });
}

export function subscribe<K extends EventName>(event: K, handler: (payload: EventMap[K]) => void): Promise<UnlistenFn> {
  if (isMockRuntime() || !isTauriRuntime()) return Promise.resolve(subscribeMock(event, handler));
  return listen<EventMap[K]>(event, ({ payload }) => handler(payload));
}

export const aztrayIpc = {
  getSnapshot,
  setConfig,
  checkEngine,
  startService,
  stopService,
  restartService,
  startAll,
  stopAll,
  restartAll,
  identifyPortOwner,
  freePort,
  getLogs,
  saveLogs,
  getConnectionString,
  clearLogs,
  quitApp,
  subscribe,
};
