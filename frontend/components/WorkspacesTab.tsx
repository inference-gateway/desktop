import { useCallback, useEffect, useMemo, useState } from "react";
import { Eraser, Pencil, RefreshCw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  api,
  type Capability,
  type ClusterConfig,
  type Machine,
  type Workspace,
  type WorkspaceConfig,
} from "@/lib/tauri";
import {
  configOf,
  draftOf,
  draftProblem,
  EMPTY_WORKSPACE,
  hasCapability,
  withCapability,
  WORKSPACE_KINDS,
  type WorkspaceDraft,
} from "@/lib/workspace-form";
import {
  advanceOnEnter,
  FIELD_CLASS,
  HINT_CLASS,
  Prerequisites,
  Problem,
  RestartNotice,
  RunningBadge,
  useCheck,
  useRestart,
} from "./ClusterParts";
import { WorkspaceVolumes } from "./WorkspaceVolumes";
import { useDesktop } from "@/store";

type Editing = { previous: string | null; draft: WorkspaceDraft };

const DAEMON_RETRIES = 3;
const DAEMON_RETRY_MS = 4000;
const AREA_CLASS =
  "w-full resize-y rounded-lg border border-input bg-transparent px-2.5 py-1.5 font-mono text-[0.8rem] text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50";
const SELECT_CLASS = "rounded-md border border-border-strong bg-card px-2.5 py-1.5 text-[0.85rem] text-foreground";

const kindLabel = (kind: string) => WORKSPACE_KINDS.find((k) => k.id === kind)?.label ?? kind;
const editable = (kind: string) => WORKSPACE_KINDS.some((k) => k.id === kind);

function reachability(
  config: WorkspaceConfig,
  known: Workspace | undefined,
  stale: boolean,
): { label: string; dot: string } {
  if (!known && stale) return { label: "Not loaded yet - restart the CLI", dot: "bg-amber-500" };
  if (!known) return { label: "The daemon has not reported this workspace", dot: "bg-muted-foreground/40" };
  if (known.reachable) return { label: "Reachable", dot: "bg-emerald-500" };
  return { label: known.error || `${kindLabel(config.kind)} is unreachable`, dot: "bg-destructive" };
}

/** Where a workspace runs: its cluster when it names one, else its own context and namespace. */
function placement(config: WorkspaceConfig): string {
  const where = [config.context, config.namespace].filter(Boolean).join(" / ");
  return config.compute ? `on ${config.compute}${where ? ` - ${where}` : ""}` : where;
}

function MachineRow({ machine, onDelete }: { machine: Machine; onDelete: () => void }) {
  const [armed, setArmed] = useState(false);
  return (
    <li className="flex items-center gap-2 text-[0.75rem] text-muted-foreground">
      <span
        className={`h-2 w-2 shrink-0 rounded-full ${machine.ready ? "bg-emerald-500" : "bg-destructive"}`}
        aria-hidden="true"
      />
      <span className="min-w-0 flex-1 truncate">
        <span className="font-mono text-foreground">{machine.name}</span>
        {` - ${machine.ready ? "Ready" : "Not ready"}${machine.reason ? ` (${machine.reason})` : ""}`}
      </span>
      <Button
        variant="outline"
        size="xs"
        aria-label={`Delete machine ${machine.name}`}
        title={armed ? "Click again to delete" : "Delete this machine - its chat starts a new one on its next message"}
        onClick={() => {
          if (armed) onDelete();
          setArmed(!armed);
        }}
        onBlur={() => setArmed(false)}
        className={armed ? "border-destructive text-destructive" : ""}
      >
        <Trash2 size={12} />
      </Button>
    </li>
  );
}

/** The machines a workspace has in its cluster, loaded when the row shows and again on demand. */
function useMachines(context: string, namespace: string, name: string, enabled: boolean) {
  const [machines, setMachines] = useState<Machine[]>([]);
  const [problem, setProblem] = useState("");
  const [deleted, setDeleted] = useState("");
  const reload = useCallback(() => {
    if (!enabled) return;
    api
      .workspaceMachines(context, namespace, name)
      .then(setMachines)
      .catch(() => setMachines([]));
  }, [enabled, context, namespace, name]);
  useEffect(reload, [reload]);
  const remove = (machine: string) =>
    api
      .deleteWorkspaceMachine(context, namespace, machine)
      .then(() => {
        setProblem("");
        setDeleted(machine);
        setMachines((prev) => prev.filter((m) => m.name !== machine));
      })
      .catch((e) => setProblem(String(e)));
  return { machines, problem, deleted, reload, remove };
}

function WorkspaceRow({
  config,
  known,
  stale,
  onEdit,
  onDelete,
}: {
  config: WorkspaceConfig;
  known: Workspace | undefined;
  stale: boolean;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const [armed, setArmed] = useState(false);
  const checkable = config.kind === "kubernetes" && Boolean(config.context);
  const { name, context, namespace } = config;
  const run = useMemo(
    () => (checkable ? () => api.checkWorkspace(context, namespace, name) : null),
    [checkable, context, namespace, name],
  );
  const { check, recheck } = useCheck(run);
  const { machines, problem, deleted, reload, remove } = useMachines(context, namespace, name, checkable);
  const state = reachability(config, known, stale);
  const where = placement(config);
  return (
    <div className="flex flex-col gap-2 rounded-lg border border-border bg-card px-4 py-3">
      <div className="flex items-center gap-3">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-[0.88rem] font-medium">
            {config.name}
            <span className="ml-2 text-[0.72rem] font-normal text-muted-foreground">{kindLabel(config.kind)}</span>
            <RunningBadge check={check} />
          </span>
          {where && <span className="truncate text-[0.75rem] text-muted-foreground">{where}</span>}
          <span className="flex items-center gap-1.5 truncate text-[0.72rem] text-muted-foreground">
            <span className={`h-2 w-2 shrink-0 rounded-full ${state.dot}`} aria-hidden="true" />
            {state.label}
          </span>
        </div>
        {checkable && (
          <Button
            variant="outline"
            size="xs"
            aria-label={`Check workspace ${config.name}`}
            title="Check the cluster's prerequisites again"
            onClick={() => {
              recheck();
              reload();
            }}
          >
            <RefreshCw size={12} />
          </Button>
        )}
        {editable(config.kind) && (
          <Button variant="outline" size="xs" aria-label={`Edit workspace ${config.name}`} onClick={onEdit}>
            <Pencil size={12} />
          </Button>
        )}
        <Button
          variant="outline"
          size="xs"
          aria-label={`Delete workspace ${config.name}`}
          title={armed ? "Click again to delete" : "Delete workspace"}
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
      {checkable && <Prerequisites checks={check?.prerequisites ?? null} />}
      {machines.length > 0 && (
        <ul className="flex flex-col gap-1.5">
          {machines.map((machine) => (
            <MachineRow key={machine.name} machine={machine} onDelete={() => remove(machine.name)} />
          ))}
        </ul>
      )}
      {deleted && (
        <p aria-live="polite" className={HINT_CLASS}>
          {`Deleted machine ${deleted}.`}
        </p>
      )}
      {problem && (
        <p role="alert" className="text-[0.75rem] text-destructive">
          {problem}
        </p>
      )}
    </div>
  );
}

function WorkspaceForm({
  editing,
  clusters,
  contexts,
  capabilities,
  onChange,
  onSave,
  onCancel,
}: {
  editing: Editing;
  clusters: ClusterConfig[];
  contexts: string[];
  capabilities: Capability[];
  onChange: (draft: WorkspaceDraft) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const { draft } = editing;
  const set = (key: keyof WorkspaceDraft) => (event: { target: { value: string } }) =>
    onChange({ ...draft, [key]: event.target.value });
  const cluster = clusters.find((c) => c.name === draft.compute);
  const ownConnection = !draft.compute;
  return (
    <div className="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
      <h3 className="text-[0.9rem] font-semibold">{editing.previous ? `Edit ${editing.previous}` : "New workspace"}</h3>
      {(clusters.length > 0 || draft.compute) && (
        <div className={FIELD_CLASS}>
          <Label htmlFor="workspace-compute">Cluster</Label>
          <select id="workspace-compute" value={draft.compute} onChange={set("compute")} className={SELECT_CLASS}>
            {clusters.map((c) => (
              <option key={c.name} value={c.name}>
                {c.name}
              </option>
            ))}
            {draft.compute && !cluster && <option value={draft.compute}>{`${draft.compute} (missing)`}</option>}
            <option value="">Its own connection</option>
          </select>
          <span className={HINT_CLASS}>
            {cluster
              ? `Runs in ${cluster.context || "the current context"} / ${cluster.namespace}, sharing the cluster's gateway. Clusters are configured under Compute.`
              : "Carries its own context, namespace and provider keys instead of a cluster's."}
          </span>
        </div>
      )}
      <div className="grid grid-cols-2 gap-3">
        {ownConnection && (
          <div className={FIELD_CLASS}>
            <Label htmlFor="workspace-kind">Type</Label>
            <select id="workspace-kind" value={draft.kind} onChange={set("kind")} className={SELECT_CLASS}>
              {WORKSPACE_KINDS.map((kind) => (
                <option key={kind.id} value={kind.id}>
                  {kind.label}
                </option>
              ))}
            </select>
          </div>
        )}
        <div className={FIELD_CLASS}>
          <Label htmlFor="workspace-name">Name</Label>
          <Input
            id="workspace-name"
            autoFocus
            value={draft.name}
            onChange={set("name")}
            onKeyDown={advanceOnEnter(ownConnection ? "workspace-context" : "workspace-image")}
            placeholder="ubuntu-desktop"
          />
        </div>
        {ownConnection && (
          <>
            <div className={FIELD_CLASS}>
              <Label htmlFor="workspace-context">Kubernetes context</Label>
              <Input
                id="workspace-context"
                list="workspace-contexts"
                value={draft.context}
                onChange={set("context")}
                onKeyDown={advanceOnEnter("workspace-namespace")}
                placeholder="k3d-computer-use"
              />
              <datalist id="workspace-contexts">
                {contexts.map((context) => (
                  <option key={context} value={context} />
                ))}
              </datalist>
            </div>
            <div className={FIELD_CLASS}>
              <Label htmlFor="workspace-namespace">Namespace</Label>
              <Input
                id="workspace-namespace"
                value={draft.namespace}
                onChange={set("namespace")}
                onKeyDown={advanceOnEnter("workspace-image")}
                placeholder="infer-workspaces"
              />
            </div>
          </>
        )}
      </div>
      <div className={FIELD_CLASS}>
        <Label htmlFor="workspace-image">Image</Label>
        <Input
          id="workspace-image"
          value={draft.image}
          onChange={set("image")}
          placeholder="ghcr.io/inference-gateway/computer:latest"
        />
        <span className={HINT_CLASS}>
          The image every session's pod runs. It needs the infer CLI, and a desktop for the switches below.
        </span>
      </div>
      <div className="grid grid-cols-3 gap-3">
        <div className={FIELD_CLASS}>
          <Label htmlFor="workspace-storage">Storage</Label>
          <Input id="workspace-storage" value={draft.storage} onChange={set("storage")} placeholder="5Gi" />
        </div>
        <div className={FIELD_CLASS}>
          <Label htmlFor="workspace-idle-ttl">Idle after</Label>
          <Input id="workspace-idle-ttl" value={draft.idle_ttl} onChange={set("idle_ttl")} placeholder="2h" />
        </div>
        <div className={FIELD_CLASS}>
          <Label htmlFor="workspace-retention">Keep suspended for</Label>
          <Input id="workspace-retention" value={draft.retention} onChange={set("retention")} placeholder="24h" />
        </div>
      </div>
      {ownConnection && (
        <div className={FIELD_CLASS}>
          <Label htmlFor="workspace-gateway-env">Provider keys</Label>
          <Input
            id="workspace-gateway-env"
            value={draft.gateway_env}
            onChange={set("gateway_env")}
            placeholder="OLLAMA_CLOUD_API_KEY"
          />
          <span className={HINT_CLASS}>
            Names only, separated by commas. The values come from API Keys and go to the workspace's gateway, never into
            the agent's pod.
          </span>
        </div>
      )}
      <div className={FIELD_CLASS}>
        <Label htmlFor="workspace-forward-env">Forwarded variables</Label>
        <Input
          id="workspace-forward-env"
          value={draft.forward_env}
          onChange={set("forward_env")}
          placeholder="GH_TOKEN"
        />
        <span className={HINT_CLASS}>Names of variables the agent itself gets, such as a token it pushes with.</span>
      </div>
      {capabilities.map((capability) => (
        <div key={capability.id} className={FIELD_CLASS}>
          <div className="flex items-center gap-3">
            <input
              type="checkbox"
              id={`workspace-capability-${capability.id}`}
              checked={hasCapability(draft, capability)}
              onChange={(event) => onChange(withCapability(draft, capability, event.target.checked))}
              className="h-4 w-4 accent-primary"
            />
            <Label htmlFor={`workspace-capability-${capability.id}`} className="cursor-pointer">
              {capability.label}
            </Label>
          </div>
          <span className={HINT_CLASS}>{capability.hint}</span>
        </div>
      ))}
      <div className={FIELD_CLASS}>
        <Label htmlFor="workspace-env">Environment</Label>
        <textarea
          id="workspace-env"
          rows={3}
          value={draft.env}
          onChange={set("env")}
          placeholder="INFER_AGENT_MODEL=ollama_cloud/glm-5.3-flash"
          className={AREA_CLASS}
        />
        <span className={HINT_CLASS}>One NAME=value per line, not for secrets.</span>
      </div>
      <div className={FIELD_CLASS}>
        <Label htmlFor="workspace-user-data">User data</Label>
        <textarea
          id="workspace-user-data"
          rows={6}
          value={draft.user_data}
          onChange={set("user_data")}
          placeholder={"#!/bin/sh\napt-get update\napt-get install -y --no-install-recommends firefox-esr"}
          spellCheck={false}
          className={`${AREA_CLASS} font-mono`}
        />
        <span className={HINT_CLASS}>
          A cloud-init script the computer runs as root when it boots, before the agent starts - to install a browser or
          other apps.
        </span>
      </div>
      <WorkspaceVolumes volumes={draft.volumes} onChange={(volumes) => onChange({ ...draft, volumes })} />
      <div className="flex gap-2">
        <Button size="sm" onClick={onSave}>
          Save workspace
        </Button>
        <Button variant="outline" size="sm" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

// The computers sessions can run on: the workspaces of ~/.infer/daemon.yaml, each on a cluster of the Compute tab or
// with its own connection. The daemon reads them when it starts, so a change asks for a CLI restart.
export function WorkspacesTab() {
  const { cleanMachines } = useDesktop();
  const [cleanArmed, setCleanArmed] = useState(false);
  const [configs, setConfigs] = useState<WorkspaceConfig[]>([]);
  const [clusters, setClusters] = useState<ClusterConfig[]>([]);
  const [image, setImage] = useState("");
  const [known, setKnown] = useState<Record<string, Workspace>>({});
  const [contexts, setContexts] = useState<string[]>([]);
  const [capabilities, setCapabilities] = useState<Capability[]>([]);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [problem, setProblem] = useState("");
  const [notice, setNotice] = useState("");

  const loadKnown = useCallback((retries: number) => {
    api
      .listWorkspaces()
      .then((list) => {
        setKnown(Object.fromEntries(list.map((workspace) => [workspace.name, workspace])));
        if (list.length === 0 && retries > 0) setTimeout(() => loadKnown(retries - 1), DAEMON_RETRY_MS);
      })
      .catch(() => {});
  }, []);

  const reload = useCallback(() => {
    api
      .workspaceConfigs()
      .then(setConfigs)
      .catch((e) => setProblem(String(e)));
    api
      .clusterConfigs()
      .then(setClusters)
      .catch(() => {});
    loadKnown(DAEMON_RETRIES);
  }, [loadKnown]);
  const { stale, setStale, restart } = useRestart(reload);

  useEffect(() => {
    reload();
    api
      .kubeContexts()
      .then(setContexts)
      .catch(() => {});
    api
      .defaultWorkspaceImage()
      .then(setImage)
      .catch(() => {});
    api
      .workspaceCapabilities()
      .then(setCapabilities)
      .catch(() => {});
  }, [reload]);

  const changed = () => {
    setProblem("");
    setStale(true);
    reload();
  };

  const save = async () => {
    if (!editing) return;
    const blocked = draftProblem(editing.draft);
    if (blocked) return setProblem(blocked);
    try {
      await api.saveWorkspace(configOf(editing.draft), editing.previous ?? undefined);
      setEditing(null);
      changed();
    } catch (e) {
      setProblem(String(e));
    }
  };

  const remove = (name: string) =>
    api
      .deleteWorkspace(name)
      .then(changed)
      .catch((e) => setProblem(String(e)));

  const clean = async () => {
    setCleanArmed(!cleanArmed);
    if (!cleanArmed) return;
    setNotice("Removing machines...");
    try {
      const removed = await cleanMachines();
      setNotice(`Removed ${removed} ${removed === 1 ? "machine" : "machines"}.`);
    } catch (e) {
      setNotice("");
      setProblem(String(e));
    }
  };

  const startNew = () =>
    setEditing({
      previous: null,
      draft: capabilities.reduce(
        (draft, capability) => withCapability(draft, capability, true),
        draftOf({ ...EMPTY_WORKSPACE, compute: clusters[0]?.name ?? "", image }),
      ),
    });

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">Workspaces</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        The computers your agents can run on. Give an agent a workspace in its editor and every new chat with it runs on
        a computer of its own there. A workspace runs on a cluster you configure under Compute.
      </p>
      {stale && <RestartNotice onRestart={restart} />}
      {notice && (
        <p aria-live="polite" className="mb-4 text-[0.8rem] text-muted-foreground">
          {notice}
        </p>
      )}
      <Problem text={problem} />
      <div className="flex flex-col gap-3">
        {configs.length === 0 && !editing && <p className="text-[0.8rem] text-muted-foreground">No workspace yet.</p>}
        {configs.map((config) => (
          <WorkspaceRow
            key={config.name}
            config={config}
            known={known[config.name]}
            stale={stale}
            onEdit={() => setEditing({ previous: config.name, draft: draftOf(config) })}
            onDelete={() => remove(config.name)}
          />
        ))}
        {editing ? (
          <WorkspaceForm
            key={editing.previous ?? "new"}
            editing={editing}
            clusters={clusters}
            contexts={contexts}
            capabilities={capabilities}
            onChange={(draft) => setEditing({ ...editing, draft })}
            onSave={save}
            onCancel={() => {
              setEditing(null);
              setProblem("");
            }}
          />
        ) : (
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="outline" size="sm" onClick={startNew}>
              New workspace
            </Button>
            <Button
              variant="outline"
              size="sm"
              aria-label="Clean up machines"
              title={
                cleanArmed
                  ? "Click again to remove all machines and their chats"
                  : "Clean up machines - removes every machine of every workspace, with the chats that live on them"
              }
              className={cleanArmed ? "border-destructive text-destructive" : ""}
              onClick={clean}
              onBlur={() => setCleanArmed(false)}
            >
              <Eraser size={14} />
            </Button>
            <span className={HINT_CLASS}>
              Clusters, Detect clusters and the one-click local cluster live under Compute.
            </span>
          </div>
        )}
      </div>
    </>
  );
}
