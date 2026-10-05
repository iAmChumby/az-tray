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
  ServiceName,
} from "./types";

function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function invokeCommand<K extends CommandName>(command: K, args: CommandArgs<K>): Promise<CommandResult<K>> {
  if (isMockRuntime() || !isTauriRuntime()) return mockInvoke(command, args);
  return invoke<CommandResult<K>>(command, args as Record<string, unknown> | undefined);
}

// --- app / settings ---------------------------------------------------------

export function getSnapshot() {
  return invokeCommand("get_snapshot", undefined);
}

export function setSettings(settings: CommandMap["set_settings"]["args"]["settings"]) {
  return invokeCommand("set_settings", { settings });
}

export function checkEngine() {
  return invokeCommand("check_engine", undefined);
}

export function quitApp(mode: CommandMap["quit_app"]["args"]["mode"]) {
  return invokeCommand("quit_app", { mode });
}

// --- instance CRUD ----------------------------------------------------------

export function listInstances() {
  return invokeCommand("list_instances", undefined);
}

export function suggestInstance(name?: string) {
  return invokeCommand("suggest_instance", name ? { name } : {});
}

export function createInstance(request: CommandMap["create_instance"]["args"]["request"]) {
  return invokeCommand("create_instance", { request });
}

export function updateInstance(request: CommandMap["update_instance"]["args"]["request"]) {
  return invokeCommand("update_instance", { request });
}

export function deleteInstance(instanceId: string) {
  return invokeCommand("delete_instance", { request: { instanceId } });
}

// --- instance lifecycle -----------------------------------------------------

export function startInstance(instanceId: string) {
  return invokeCommand("start_instance", { instanceId });
}

export function stopInstance(instanceId: string) {
  return invokeCommand("stop_instance", { instanceId });
}

export function restartInstance(instanceId: string) {
  return invokeCommand("restart_instance", { instanceId });
}

// --- one service within an instance ----------------------------------------

export function startService(instanceId: string, serviceName: ServiceName) {
  return invokeCommand("start_service", { instanceId, serviceName });
}

export function stopService(instanceId: string, serviceName: ServiceName) {
  return invokeCommand("stop_service", { instanceId, serviceName });
}

export function restartService(instanceId: string, serviceName: ServiceName) {
  return invokeCommand("restart_service", { instanceId, serviceName });
}

// --- every instance ---------------------------------------------------------

export function startAll() {
  return invokeCommand("start_all", undefined);
}

export function stopAll() {
  return invokeCommand("stop_all", undefined);
}

export function restartAll() {
  return invokeCommand("restart_all", undefined);
}

// --- ports ------------------------------------------------------------------

export function identifyPortOwner(instanceId: string, serviceName: ServiceName) {
  return invokeCommand("identify_port_owner", { instanceId, serviceName });
}

export function freePort(args: CommandMap["free_port"]["args"]) {
  return invokeCommand("free_port", args);
}

// --- logs -------------------------------------------------------------------

export function getLogs(args: CommandMap["get_logs"]["args"] = {}) {
  return invokeCommand("get_logs", args);
}

export function saveLogs(args: CommandMap["save_logs"]["args"] = {}) {
  return invokeCommand("save_logs", args);
}

export function clearLogs(instanceId?: string, serviceName?: ServiceName) {
  return invokeCommand("clear_logs", { ...(instanceId ? { instanceId } : {}), ...(serviceName ? { serviceName } : {}) });
}

export function getAppLog(limit?: number) {
  return invokeCommand("get_app_log", limit === undefined ? {} : { limit });
}

// --- connection provisioning ------------------------------------------------

export function getConnectionInfo(instanceId: string) {
  return invokeCommand("get_connection_info", { instanceId });
}

/** Omit serviceName for the combined Blob+Queue+Table connection string. */
export function getConnectionString(instanceId: string, serviceName?: ServiceName) {
  return invokeCommand("get_connection_string", serviceName ? { instanceId, serviceName } : { instanceId });
}

// --- MCP --------------------------------------------------------------------

export function getMcpStatus() {
  return invokeCommand("get_mcp_status", undefined);
}

export function restartMcp() {
  return invokeCommand("restart_mcp", undefined);
}

// --- events -----------------------------------------------------------------

export function subscribe<K extends EventName>(event: K, handler: (payload: EventMap[K]) => void): Promise<UnlistenFn> {
  if (isMockRuntime() || !isTauriRuntime()) return Promise.resolve(subscribeMock(event, handler));
  return listen<EventMap[K]>(event, ({ payload }) => handler(payload));
}

export const aztrayIpc = {
  getSnapshot,
  setSettings,
  checkEngine,
  quitApp,
  listInstances,
  suggestInstance,
  createInstance,
  updateInstance,
  deleteInstance,
  startInstance,
  stopInstance,
  restartInstance,
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
  clearLogs,
  getAppLog,
  getConnectionInfo,
  getConnectionString,
  getMcpStatus,
  restartMcp,
  subscribe,
};
