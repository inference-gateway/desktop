import type { McpServerConfig, McpStatus, Secret } from "./tauri";

export const EMPTY_SERVER: McpServerConfig = {
  name: "",
  enabled: true,
  run: false,
  url: "",
  headers: [],
  image: "",
  port: 0,
  workspaces: [],
};

/** A server's connection as its row says it, from `infer mcp status`. */
export function serverState(server: McpServerConfig, status: McpStatus | null): string {
  if (!server.enabled) return "Off on this computer";
  if (!status) return "Checking...";
  if (!status.enabled) return "MCP is off";
  const row = status.servers.find((r) => r.name === server.name);
  if (!row) return "Not checked";
  if (row.connected) return `Connected, ${row.tools} ${row.tools === 1 ? "tool" : "tools"}`;
  return row.error ? `Not connected: ${row.error}` : "Not connected";
}

/** Where a server goes that has no secret for a header's variable. A variable no secret provides is an API key's,
 * which this computer always has, so it is never reported. */
export function missingSecrets(server: McpServerConfig, secrets: Secret[]): string[] {
  const variables = [...new Set(server.headers.map((h) => h.variable).filter(Boolean))];
  return variables.flatMap((variable) => {
    const providers = secrets.filter((s) => s.variable === variable);
    if (providers.length === 0) return [];
    const places = [
      ...(server.enabled && !providers.some((s) => s.computer) ? ["this computer"] : []),
      ...server.workspaces.filter((w) => !providers.some((s) => s.workspaces.includes(w))).map((w) => `workspace ${w}`),
    ];
    return places.map((place) => `${place} gets no ${variable}`);
  });
}

/** The variables a header can pick, one per secret variable, sorted. */
export function secretVariables(secrets: Secret[]): string[] {
  return [...new Set(secrets.map((s) => s.variable))].sort();
}
