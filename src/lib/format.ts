import type { ConnectionInfo, McpStatus } from "./types";

export function formatUptime(seconds: number | null | undefined) {
  if (seconds === null || seconds === undefined || seconds < 0) return "-";
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = Math.floor(seconds % 60);
  return hours ? `${hours}h ${String(minutes).padStart(2, "0")}m` : `${minutes}m ${String(secs).padStart(2, "0")}s`;
}

export function formatClock(value: string | null | undefined) {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleTimeString([], { hour12: false });
}

export function formatDateTime(value: string | null | undefined) {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString([], { hour12: false });
}

export function pluralize(count: number, one: string, many = `${one}s`) {
  return `${count} ${count === 1 ? one : many}`;
}

// --- Connection snippets -----------------------------------------------------

export type SnippetFormat = "env" | "powershell" | "appsettings" | "localSettings";

export const SNIPPET_LABELS: Record<SnippetFormat, string> = {
  env: ".env",
  powershell: "PowerShell",
  appsettings: "appsettings.json",
  localSettings: "local.settings.json",
};

export function connectionSnippet(format: SnippetFormat, info: ConnectionInfo): string {
  const value = info.connectionString;
  switch (format) {
    case "env":
      return `AZURE_STORAGE_CONNECTION_STRING=${value}`;
    case "powershell":
      return `$env:AZURE_STORAGE_CONNECTION_STRING = "${value}"`;
    case "appsettings":
      return JSON.stringify({ ConnectionStrings: { AzureStorage: value } }, null, 2);
    case "localSettings":
      return JSON.stringify({ IsEncrypted: false, Values: { AzureWebJobsStorage: value } }, null, 2);
  }
}

// --- MCP snippets ------------------------------------------------------------

export function claudeConfigSnippet(status: McpStatus): string {
  return `"az-tray": { "type": "http", "url": "${status.url}" }`;
}

export function claudeCliCommand(status: McpStatus): string {
  return `claude mcp add --transport http az-tray ${status.url}`;
}
