import { useCallback, useEffect, useState } from "react";
import { Pencil, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, type Secret, type WorkspaceConfig } from "@/lib/tauri";
import { advanceOnEnter, FIELD_CLASS, HINT_CLASS, RestartNotice } from "./ClusterParts";
import { useDesktop } from "@/store";

const EMPTY_SECRET: Secret = { label: "", variable: "", computer: false, workspaces: [], mcp_servers: [] };

type Editing = { isNew: boolean; secret: Secret; value: string };

const assigned = (secret: Secret | null) => !!secret && (secret.computer || secret.workspaces.length > 0);

function SecretRow({ secret, onEdit, onDelete }: { secret: Secret; onEdit: () => void; onDelete: () => void }) {
  const [armed, setArmed] = useState(false);
  const places = [...(secret.computer ? ["this computer"] : []), ...secret.workspaces.map((w) => `workspace ${w}`)];
  return (
    <div className="flex items-center gap-3 rounded-lg border border-border bg-card px-4 py-3">
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-[0.88rem] font-medium">
          {secret.label}
          <span className="ml-2 font-mono text-[0.72rem] font-normal text-muted-foreground">{secret.variable}</span>
        </span>
        <span className="truncate text-[0.75rem] text-muted-foreground">
          {places.length > 0 ? `Goes to ${places.join(", ")}` : "Assigned nowhere, so no process gets it"}
        </span>
        {secret.mcp_servers.length > 0 && (
          <span className="truncate text-[0.75rem] text-muted-foreground">
            {`Read by the MCP servers ${secret.mcp_servers.join(", ")}`}
          </span>
        )}
      </div>
      <Button variant="outline" size="xs" aria-label={`Edit secret ${secret.label}`} onClick={onEdit}>
        <Pencil size={12} />
      </Button>
      <Button
        variant="outline"
        size="xs"
        aria-label={`Delete secret ${secret.label}`}
        title={armed ? "Click again to delete" : "Delete secret"}
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
  );
}

function SecretForm({
  editing,
  workspaces,
  saving,
  onChange,
  onSave,
  onCancel,
}: {
  editing: Editing;
  workspaces: WorkspaceConfig[];
  saving: boolean;
  onChange: (editing: Editing) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const { secret, isNew } = editing;
  const set = (patch: Partial<Secret>) => onChange({ ...editing, secret: { ...secret, ...patch } });
  const toggle = (name: string, on: boolean) =>
    set({ workspaces: on ? [...secret.workspaces, name] : secret.workspaces.filter((w) => w !== name) });
  return (
    <div className="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
      <h3 className="text-[0.9rem] font-semibold">{isNew ? "New secret" : `Edit ${secret.label}`}</h3>
      <div className="grid grid-cols-2 gap-3">
        <div className={FIELD_CLASS}>
          <Label htmlFor="secret-label">Label</Label>
          <Input
            id="secret-label"
            autoFocus={isNew}
            disabled={!isNew}
            value={secret.label}
            onChange={(e) => set({ label: e.target.value })}
            onKeyDown={advanceOnEnter("secret-variable")}
            placeholder="code-reviewer-inbox"
          />
        </div>
        <div className={FIELD_CLASS}>
          <Label htmlFor="secret-variable">Variable</Label>
          <Input
            id="secret-variable"
            autoFocus={!isNew}
            value={secret.variable}
            onChange={(e) => set({ variable: e.target.value })}
            onKeyDown={advanceOnEnter("secret-value")}
            placeholder="AGENTMAIL_API_KEY"
            className="font-mono"
          />
        </div>
      </div>
      <div className={FIELD_CLASS}>
        <Label htmlFor="secret-value">Value</Label>
        <Input
          id="secret-value"
          type="password"
          autoComplete="off"
          value={editing.value}
          onChange={(e) => onChange({ ...editing, value: e.target.value })}
          placeholder={isNew ? "" : "Leave blank to keep the saved value"}
        />
        <span className={HINT_CLASS}>
          Kept in ~/.infer/auth.yaml, readable by you alone. It is never shown again and never travels in an export.
        </span>
      </div>
      <fieldset className="flex flex-col gap-1.5">
        <legend className="mb-1 text-[0.8rem] font-medium">Goes to</legend>
        <label className="flex items-center gap-2 text-[0.82rem]">
          <input
            type="checkbox"
            id="secret-computer"
            checked={secret.computer}
            onChange={(e) => set({ computer: e.target.checked })}
            className="h-4 w-4 accent-primary"
          />
          This computer
        </label>
        {workspaces.map((workspace) => (
          <label key={workspace.name} className="flex items-center gap-2 text-[0.82rem]">
            <input
              type="checkbox"
              id={`secret-workspace-${workspace.name}`}
              checked={secret.workspaces.includes(workspace.name)}
              onChange={(e) => toggle(workspace.name, e.target.checked)}
              className="h-4 w-4 accent-primary"
            />
            {`Workspace ${workspace.name}`}
          </label>
        ))}
        <span className={HINT_CLASS}>
          This computer passes it to every infer process the app starts, the local daemon's workers too. A Kubernetes
          workspace's pod reads it from the Secret {`infer-secret-${secret.label || "<label>"}`} in its namespace, so
          the value never passes through a session.
        </span>
      </fieldset>
      <div className="flex gap-2">
        <Button size="sm" disabled={saving} onClick={onSave}>
          Save secret
        </Button>
        <Button variant="outline" size="sm" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

// The secrets agents' computers need, kept once in ~/.infer/auth.yaml and assigned where they go. This computer's
// processes and the daemon's workspaces read them when they start, so a change asks for a CLI restart.
export function SecretsTab() {
  const { restartBackend } = useDesktop();
  const [secrets, setSecrets] = useState<Secret[]>([]);
  const [workspaces, setWorkspaces] = useState<WorkspaceConfig[]>([]);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [problem, setProblem] = useState("");
  const [stale, setStale] = useState(false);
  const [saving, setSaving] = useState(false);

  const reload = useCallback(() => {
    api
      .listSecrets()
      .then(setSecrets)
      .catch((e) => setProblem(String(e)));
    api
      .workspaceConfigs()
      .then((all) => setWorkspaces(all.filter((w) => w.kind === "kubernetes")))
      .catch(() => {});
  }, []);

  useEffect(reload, [reload]);

  const settle = (done: string, failures: string[], touched: boolean) => {
    setProblem(failures.length > 0 ? `${done}, but not every cluster took it: ${failures.join("; ")}` : "");
    if (touched) setStale(true);
    reload();
  };

  const save = async () => {
    if (!editing || saving) return;
    const secret = {
      ...editing.secret,
      label: editing.secret.label.trim(),
      variable: editing.secret.variable.trim(),
    };
    const before = secrets.find((s) => s.label === secret.label) ?? null;
    setSaving(true);
    try {
      const failures = await api.saveSecret(secret, editing.value, editing.isNew);
      setEditing(null);
      settle("Saved", failures, assigned(before) || assigned(secret));
    } catch (e) {
      setProblem(String(e));
    } finally {
      setSaving(false);
    }
  };

  const remove = (secret: Secret) =>
    api
      .deleteSecret(secret.label)
      .then((failures) => settle("Deleted", failures, assigned(secret)))
      .catch((e) => setProblem(String(e)));

  const restart = async () => {
    await restartBackend(false);
    setStale(false);
    reload();
  };

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">Secrets</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        Values your agents' computers need, each kept once and given to the computers you pick, where it becomes its
        variable. Secrets may share a variable as long as each computer gets one of them, so every agent can mail as its
        own inbox through AGENTMAIL_API_KEY.
      </p>
      {stale && (
        <>
          <RestartNotice onRestart={restart} />
          <p className={`-mt-2 mb-4 ${HINT_CLASS}`}>
            Sessions already running on a workspace keep the old values until they get a new machine.
          </p>
        </>
      )}
      {problem && (
        <p role="alert" className="mb-4 text-[0.8rem] text-destructive">
          {problem}
        </p>
      )}
      <div className="flex flex-col gap-3">
        {secrets.length === 0 && !editing && <p className="text-[0.8rem] text-muted-foreground">No secret yet.</p>}
        {secrets.map((secret) => (
          <SecretRow
            key={secret.label}
            secret={secret}
            onEdit={() => setEditing({ isNew: false, secret, value: "" })}
            onDelete={() => remove(secret)}
          />
        ))}
        {editing ? (
          <SecretForm
            key={editing.isNew ? "new" : editing.secret.label}
            editing={editing}
            workspaces={workspaces}
            saving={saving}
            onChange={setEditing}
            onSave={save}
            onCancel={() => {
              setEditing(null);
              setProblem("");
            }}
          />
        ) : (
          <div>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setEditing({ isNew: true, secret: EMPTY_SECRET, value: "" })}
            >
              New secret
            </Button>
          </div>
        )}
      </div>
    </>
  );
}
