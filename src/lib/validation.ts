import { SERVICE_NAMES, SERVICE_LABELS, type InstanceSnapshot, type McpConfig, type ServiceName } from "./types";

export type PortProblems = Partial<Record<ServiceName, string>>;

/** Client-side mirror of the engine's port rules, so the form can warn before saving. */
export function portProblems(
  ports: Record<ServiceName, string>,
  instances: InstanceSnapshot[],
  mcp: McpConfig | undefined,
  excludeId?: string,
): PortProblems {
  const problems: PortProblems = {};
  const parsed = {} as Record<ServiceName, number>;
  for (const name of SERVICE_NAMES) {
    const value = Number(ports[name]);
    parsed[name] = value;
    if (!Number.isInteger(value) || value < 1 || value > 65535) problems[name] = "Use a port from 1 to 65535";
  }
  for (const name of SERVICE_NAMES) {
    if (problems[name]) continue;
    const twin = SERVICE_NAMES.find((other) => other !== name && parsed[other] === parsed[name]);
    if (twin) { problems[name] = `Same as the ${SERVICE_LABELS[twin]} port`; continue; }
    const owner = instances.find((item) => item.config.id !== excludeId && SERVICE_NAMES.some((s) => item.config.ports[s] === parsed[name]));
    if (owner) { problems[name] = `Already used by "${owner.config.name}"`; continue; }
    if (mcp && parsed[name] >= mcp.port && parsed[name] <= mcp.port + 9) problems[name] = `Reserved for the MCP server (${mcp.port}-${mcp.port + 9})`;
  }
  return problems;
}

export function nameProblem(name: string, instances: InstanceSnapshot[], excludeId?: string): string | null {
  const trimmed = name.trim();
  if (!trimmed) return "Give the instance a name";
  if (trimmed.length > 48) return "Use 48 characters or fewer";
  if (instances.some((item) => item.config.id !== excludeId && item.config.name.toLowerCase() === trimmed.toLowerCase())) return "Another instance already has this name";
  return null;
}
