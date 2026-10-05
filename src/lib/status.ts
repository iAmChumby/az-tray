import type { InstanceState, McpStatus, ServiceState } from "./types";

/** One visual vocabulary for every status in the app (services, instances, MCP). */
export type Tone = "running" | "starting" | "warning" | "broken" | "occupied" | "stopped" | "disabled";

export const TONE_DOT: Record<Tone, string> = {
  running: "bg-status-running",
  starting: "bg-status-starting az-pulse",
  warning: "bg-status-starting",
  broken: "bg-status-broken",
  occupied: "bg-status-occupied",
  stopped: "bg-status-stopped",
  disabled: "bg-transparent ring-1 ring-inset ring-status-stopped",
};

export const TONE_TEXT: Record<Tone, string> = {
  running: "text-status-running",
  starting: "text-status-starting",
  warning: "text-status-starting",
  broken: "text-status-broken",
  occupied: "text-status-occupied",
  stopped: "text-muted-foreground",
  disabled: "text-muted-foreground",
};

export const TONE_SOFT: Record<Tone, string> = {
  running: "bg-status-running/12 text-status-running",
  starting: "bg-status-starting/14 text-status-starting",
  warning: "bg-status-starting/14 text-status-starting",
  broken: "bg-status-broken/14 text-status-broken",
  occupied: "bg-status-occupied/14 text-status-occupied",
  stopped: "bg-foreground/6 text-muted-foreground",
  disabled: "bg-foreground/6 text-muted-foreground",
};

export function serviceTone(state: ServiceState): Tone {
  switch (state) {
    case "running": return "running";
    case "starting": return "starting";
    case "broken": return "broken";
    case "portInUse": return "occupied";
    default: return "stopped";
  }
}

export function serviceStateLabel(state: ServiceState): string {
  switch (state) {
    case "portInUse": return "Port in use";
    case "broken": return "Failed";
    default: return state[0].toUpperCase() + state.slice(1);
  }
}

export function instanceTone(state: InstanceState): Tone {
  switch (state) {
    case "running": return "running";
    case "starting": return "starting";
    case "partial": return "warning";
    case "broken": return "broken";
    default: return "stopped";
  }
}

export function instanceStateLabel(state: InstanceState): string {
  switch (state) {
    case "broken": return "Needs attention";
    case "partial": return "Partly running";
    default: return state[0].toUpperCase() + state.slice(1);
  }
}

export type McpPhase = "running" | "starting" | "failed" | "disabled";

/** disabled / running / failed / starting (enabled, not yet bound, no error yet). */
export function mcpPhase(status: McpStatus | null | undefined): McpPhase {
  if (!status) return "starting";
  if (!status.enabled) return "disabled";
  if (status.running) return "running";
  return status.error ? "failed" : "starting";
}

export function mcpTone(phase: McpPhase): Tone {
  return phase === "running" ? "running" : phase === "failed" ? "broken" : phase === "starting" ? "starting" : "disabled";
}

export function mcpLabel(phase: McpPhase): string {
  return phase === "running" ? "Running" : phase === "failed" ? "Failed" : phase === "starting" ? "Starting" : "Disabled";
}
