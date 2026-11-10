import { useCallback, useEffect, useState } from "react";
import { useBackend } from "@/stores/backend";
import { Pencil, Plus, RefreshCw, Trash2, TriangleAlert, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  api,
  type McpHeader,
  type McpServerConfig,
  type McpStatus,
  type Secret,
  type WorkspaceConfig,
} from "@/lib/tauri";
import { EMPTY_SERVER, missingSecrets, secretVariables, serverState } from "@/lib/mcp-form";
import { advanceOnEnter, FIELD_CLASS, HINT_CLASS, RestartNotice } from "./ClusterParts";

type Editing = { isNew: boolean; server: McpServerConfig };

const SELECT_CLASS =
  "h-8 rounded-lg border border-input bg-transparent px-2.5 text-[0.85rem] text-foreground outline-none focus-visible:border-ring";

function ServerRow({
  server,
  status,
  secrets,
  onEdit,
  onDelete,
}: {
  server: McpServerConfig;
  status: McpStatus | null;
  secrets: Secret[];
  onEdit: () => void;
  onDelete: () => void;
}) {
  const [armed, setArmed] = useState(false);
  const places = [...(server.enabled ? ["this computer"] : []), ...server.workspaces.map((w) => `workspace ${w}`)];
  const missing = missingSecrets(server, secrets);
  return (
    <div className="flex flex-col gap-1.5 rounded-lg border border-border bg-card px-4 py-3">
      <div className="flex items-center gap-3">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-[0.88rem] font-medium">
            {server.name}
            <span className="ml-2 text-[0.72rem] font-normal text-muted-foreground">
              {server.run ? "Container" : "Hosted"}
            </span>
            <span className="ml-2 text-[0.72rem] font-normal text-muted-foreground">{serverState(server, status)}</span>
          </span>
          <span className="truncate text-[0.75rem] text-muted-foreground">
            {server.run ? `${server.image} on port ${server.port}` : server.url}
          </span>
          <span className="truncate text-[0.75rem] text-muted-foreground">
            {places.length > 0 ? `Goes to ${places.join(", ")}` : "Goes nowhere"}
          </span>
        </div>
        <Button variant="outline" size="xs" aria-label={`Edit server ${server.name}`} onClick={onEdit}>
          <Pencil size={12} />
        </Button>
        <Button
          variant="outline"
          size="xs"
          aria-label={`Delete server ${server.name}`}
          title={armed ? "Click again to delete" : "Delete server"}
          onClick={() => {
            if (armed) onDelete();
            setArmed(!armed);
          }}
          onBlur={() => setArmed(false)}
          className={armed ? "border-destructive text-destructive" : ""}
        >
          <Trash2 size={12} />
        </Button>
      </div>
      {missing.length > 0 && (
        <span className="flex items-center gap-1.5 text-[0.72rem] text-amber-600 dark:text-amber-400">
          <TriangleAlert size={12} aria-hidden="true" />
          {`${missing.join(", ")}. Assign one under Secrets.`}
        </span>
      )}
    </div>
  );
}

function HeaderRow({
  header,
  index,
  variables,
  onChange,
  onRemove,
}: {
  header: McpHeader;
  index: number;
  variables: string[];
  onChange: (header: McpHeader) => void;
  onRemove: () => void;
}) {
  const written = header.literal && !header.variable;
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-2">
        <Input
          id={`mcp-header-name-${index}`}
          aria-label={`Header ${index + 1} name`}
          value={header.name}
          onChange={(e) => onChange({ ...header, name: e.target.value })}
          placeholder="x-api-key"
        />
        <Input
          id={`mcp-header-prefix-${index}`}
          aria-label={`Header ${index + 1} prefix`}
          value={header.prefix}
          disabled={written}
          onChange={(e) => onChange({ ...header, prefix: e.target.value })}
          placeholder="Bearer (optional)"
        />
        <select
          id={`mcp-header-variable-${index}`}
          aria-label={`Header ${index + 1} secret`}
          value={header.variable}
          onChange={(e) => onChange({ ...header, literal: false, variable: e.target.value })}
          className={SELECT_CLASS}
        >
          <option value="">Pick a secret</option>
          {header.variable && !variables.includes(header.variable) && (
            <option value={header.variable}>{header.variable}</option>
          )}
          {variables.map((variable) => (
            <option key={variable} value={variable}>
              {variable}
            </option>
          ))}
        </select>
        <Button variant="outline" size="xs" aria-label={`Remove header ${index + 1}`} onClick={onRemove}>
          <X size={12} />
        </Button>
      </div>
      {written && (
        <span className={HINT_CLASS}>
          Its value sits in mcp.yaml as written. Pick a secret to replace it, or remove the header.
        </span>
      )}
    </div>
  );
}

function ServerForm({
  editing,
  workspaces,
  variables,
  saving,
  onChange,
  onSave,
  onCancel,
}: {
  editing: Editing;
  workspaces: WorkspaceConfig[];
  variables: string[];
  saving: boolean;
  onChange: (editing: Editing) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const { server, isNew } = editing;
  const set = (patch: Partial<McpServerConfig>) => onChange({ ...editing, server: { ...server, ...patch } });
  const setHeader = (index: number, header: McpHeader) =>
    set({ headers: server.headers.map((h, i) => (i === index ? header : h)) });
  const toggle = (name: string, on: boolean) =>
    set({ workspaces: on ? [...server.workspaces, name] : server.workspaces.filter((w) => w !== name) });
  const kind = server.run ? "container" : "hosted";
  return (
    <div className="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
      <h3 className="text-[0.9rem] font-semibold">{isNew ? `New ${kind} server` : `Edit ${server.name}`}</h3>
      <div className={FIELD_CLASS}>
        <Label htmlFor="mcp-name">Name</Label>
        <Input
          id="mcp-name"
          autoFocus={isNew}
          disabled={!isNew}
          value={server.name}
          onChange={(e) => set({ name: e.target.value })}
          onKeyDown={advanceOnEnter(server.run ? "mcp-image" : "mcp-url")}
          placeholder={server.run ? "filesystem" : "agentmail"}
        />
      </div>
      {server.run ? (
        <div className="grid grid-cols-[1fr_8rem] gap-3">
          <div className={FIELD_CLASS}>
            <Label htmlFor="mcp-image">Image</Label>
            <Input
              id="mcp-image"
              value={server.image}
              onChange={(e) => set({ image: e.target.value })}
              onKeyDown={advanceOnEnter("mcp-port")}
              placeholder="mcp/filesystem:latest"
            />
          </div>
          <div className={FIELD_CLASS}>
            <Label htmlFor="mcp-port">Port</Label>
            <Input
              id="mcp-port"
              type="number"
              min={1}
              max={65535}
              value={server.port || ""}
              onChange={(e) => set({ port: Number(e.target.value) || 0 })}
              placeholder="3000"
            />
          </div>
        </div>
      ) : (
        <>
          <div className={FIELD_CLASS}>
            <Label htmlFor="mcp-url">URL</Label>
            <Input
              id="mcp-url"
              autoFocus={!isNew}
              value={server.url}
              onChange={(e) => set({ url: e.target.value })}
              placeholder="https://mcp.agentmail.to/mcp"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <span className="text-[0.8rem] font-medium">Headers</span>
            {server.headers.map((header, index) => (
              <HeaderRow
                key={index}
                header={header}
                index={index}
                variables={variables}
                onChange={(next) => setHeader(index, next)}
                onRemove={() => set({ headers: server.headers.filter((_, i) => i !== index) })}
              />
            ))}
            <div>
              <Button
                variant="outline"
                size="xs"
                onClick={() =>
                  set({ headers: [...server.headers, { name: "", prefix: "", variable: "", literal: false }] })
                }
              >
                <Plus size={12} />
                Add header
              </Button>
            </div>
            <span className={HINT_CLASS}>
              A header's value is a secret from the Secrets tab, written into mcp.yaml as {"${VARIABLE}"} and expanded
              by each computer from the secret it was given.
            </span>
          </div>
        </>
      )}
      <fieldset className="flex flex-col gap-1.5">
        <legend className="mb-1 text-[0.8rem] font-medium">Goes to</legend>
        <label className="flex items-center gap-2 text-[0.82rem]">
          <input
            type="checkbox"
            id="mcp-computer"
            checked={server.enabled}
            onChange={(e) => set({ enabled: e.target.checked })}
            className="h-4 w-4 accent-primary"
          />
          This computer
        </label>
        {!server.run &&
          workspaces.map((workspace) => (
            <label key={workspace.name} className="flex items-center gap-2 text-[0.82rem]">
              <input
                type="checkbox"
                id={`mcp-workspace-${workspace.name}`}
                checked={server.workspaces.includes(workspace.name)}
                onChange={(e) => toggle(workspace.name, e.target.checked)}
                className="h-4 w-4 accent-primary"
              />
              {`Workspace ${workspace.name}`}
            </label>
          ))}
        <span className={HINT_CLASS}>
          {server.run
            ? "A container server runs in Docker on this computer only."
            : "A Kubernetes workspace's pods get it once the infer CLI and the operator pass MCP servers to them. Give the workspace a secret for each header under Secrets."}
        </span>
      </fieldset>
      <div className="flex gap-2">
        <Button size="sm" disabled={saving} onClick={onSave}>
          Save server
        </Button>
        <Button variant="outline" size="sm" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

// The MCP servers of ~/.infer/mcp.yaml, edited as written so a ${VARIABLE} reference stays one. New chats pick up a
// change, and a workspace's servers live in daemon.yaml, which asks for a CLI restart.
export function McpTab() {
  const { restartBackend } = useBackend();
  const [enabled, setEnabled] = useState(false);
  const [servers, setServers] = useState<McpServerConfig[]>([]);
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [secrets, setSecrets] = useState<Secret[]>([]);
  const [workspaces, setWorkspaces] = useState<WorkspaceConfig[]>([]);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [problem, setProblem] = useState("");
  const [stale, setStale] = useState(false);
  const [saving, setSaving] = useState(false);

  const check = useCallback(() => {
    setStatus(null);
    api
      .mcpStatus()
      .then(setStatus)
      .catch((e) => setProblem(String(e)));
  }, []);

  const reload = useCallback(() => {
    api
      .mcpSettings()
      .then((settings) => {
        setEnabled(settings.enabled);
        setServers(settings.servers);
      })
      .catch((e) => setProblem(String(e)));
    api
      .listSecrets()
      .then(setSecrets)
      .catch(() => {});
    api
      .workspaceConfigs()
      .then((all) => setWorkspaces(all.filter((w) => w.kind === "kubernetes")))
      .catch(() => {});
    check();
  }, [check]);

  useEffect(reload, [reload]);

  const settle = (done: string, [listed, failures]: [boolean, string[]]) => {
    setProblem(failures.length > 0 ? `${done}, but its container did not follow: ${failures.join("; ")}` : "");
    if (listed) setStale(true);
    reload();
  };

  const save = async () => {
    if (!editing || saving) return;
    const server = {
      ...editing.server,
      name: editing.server.name.trim(),
      url: editing.server.url.trim(),
      image: editing.server.image.trim(),
    };
    setSaving(true);
    try {
      const outcome = await api.saveMcpServer(server, editing.isNew);
      setEditing(null);
      settle("Saved", outcome);
    } catch (e) {
      setProblem(String(e));
    } finally {
      setSaving(false);
    }
  };

  const remove = (name: string) =>
    api
      .deleteMcpServer(name)
      .then((outcome) => settle("Deleted", outcome))
      .catch((e) => setProblem(String(e)));

  const toggleAll = (on: boolean) =>
    api
      .setMcpEnabled(on)
      .then(reload)
      .catch((e) => setProblem(String(e)));

  const restart = async () => {
    await restartBackend(false);
    setStale(false);
    reload();
  };

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">MCPs</h2>
      <p className="mb-4 text-[0.8rem] text-muted-foreground">
        The MCP servers of ~/.infer/mcp.yaml. A hosted server authenticates with headers whose values are secrets, and
        an agent can pick the servers it uses in its editor. New chats pick up a change, running chats keep the servers
        they started with.
      </p>
      <div className="mb-4 flex items-center gap-3">
        <input
          type="checkbox"
          id="mcp-enabled"
          checked={enabled}
          onChange={(e) => toggleAll(e.target.checked)}
          className="h-4 w-4 accent-primary"
        />
        <Label htmlFor="mcp-enabled" className="flex-1">
          Use MCP servers on this computer
        </Label>
        <Button
          variant="outline"
          size="xs"
          aria-label="Check MCP servers"
          title="Check the servers again"
          onClick={check}
        >
          <RefreshCw size={12} />
        </Button>
      </div>
      {stale && <RestartNotice onRestart={restart} />}
      {problem && (
        <p role="alert" className="mb-4 text-[0.8rem] text-destructive">
          {problem}
        </p>
      )}
      <div className="flex flex-col gap-3">
        {servers.length === 0 && !editing && <p className="text-[0.8rem] text-muted-foreground">No MCP server yet.</p>}
        {servers.map((server) => (
          <ServerRow
            key={server.name}
            server={server}
            status={status}
            secrets={secrets}
            onEdit={() => setEditing({ isNew: false, server })}
            onDelete={() => remove(server.name)}
          />
        ))}
        {editing ? (
          <ServerForm
            key={editing.isNew ? `new-${editing.server.run}` : editing.server.name}
            editing={editing}
            workspaces={workspaces}
            variables={secretVariables(secrets)}
            saving={saving}
            onChange={setEditing}
            onSave={save}
            onCancel={() => {
              setEditing(null);
              setProblem("");
            }}
          />
        ) : (
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" size="sm" onClick={() => setEditing({ isNew: true, server: EMPTY_SERVER })}>
              New hosted server
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setEditing({ isNew: true, server: { ...EMPTY_SERVER, run: true } })}
            >
              New container server
            </Button>
          </div>
        )}
      </div>
    </>
  );
}
