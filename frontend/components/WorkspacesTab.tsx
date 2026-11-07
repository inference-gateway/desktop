import { useCallback, useEffect, useState, type KeyboardEvent } from "react";
import { Check, CircleCheck, Container, Eraser, Pencil, RefreshCw, Trash2, TriangleAlert, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, type ClusterCheck, type Prerequisite, type Workspace, type WorkspaceConfig } from "@/lib/tauri";
import {
  configOf,
  draftOf,
  draftProblem,
  EMPTY_WORKSPACE,
  WORKSPACE_KINDS,
  type WorkspaceDraft,
} from "@/lib/workspace-form";
import { useDesktop } from "@/store";

type Editing = { previous: string | null; draft: WorkspaceDraft };

const DAEMON_RETRIES = 3;
const DAEMON_RETRY_MS = 4000;
const FIELD_CLASS = "flex flex-col gap-1";
const HINT_CLASS = "text-[0.72rem] text-muted-foreground";
const AREA_CLASS =
  "w-full resize-y rounded-lg border border-input bg-transparent px-2.5 py-1.5 font-mono text-[0.8rem] text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50";

const kindLabel = (kind: string) => WORKSPACE_KINDS.find((k) => k.id === kind)?.label ?? kind;
const editable = (kind: string) => WORKSPACE_KINDS.some((k) => k.id === kind);

function advanceOnEnter(nextId: string) {
  return (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    document.getElementById(nextId)?.focus();
  };
}

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

/** The cluster prerequisites of a Kubernetes workspace, checked when the row shows and again on demand. */
function useClusterCheck(config: WorkspaceConfig): { check: ClusterCheck | null; recheck: () => void } {
  const [check, setCheck] = useState<ClusterCheck | null>(null);
  const { kind, context, namespace } = config;
  const recheck = useCallback(() => {
    if (kind !== "kubernetes" || !context) return;
    setCheck(null);
    api
      .checkWorkspace(context, namespace)
      .then(setCheck)
      .catch((e) =>
        setCheck({
          prerequisites: [{ name: "Cluster reachable", met: false, detail: String(e) }],
          running: null,
          suspended: null,
        }),
      );
  }, [kind, context, namespace]);
  useEffect(recheck, [recheck]);
  return { check, recheck };
}

function RunningBadge({ check }: { check: ClusterCheck | null }) {
  if (!check || check.running === null) return null;
  const active = check.running > 0;
  return (
    <span
      title={`${check.suspended ?? 0} suspended`}
      className={`ml-2 rounded-full px-2 py-0.5 text-[0.7rem] font-medium ${
        active ? "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400" : "bg-foreground/5 text-muted-foreground"
      }`}
    >
      {`${check.running} running`}
    </span>
  );
}

function Prerequisites({ checks }: { checks: Prerequisite[] | null }) {
  if (!checks) return <span className={HINT_CLASS}>Checking the cluster...</span>;
  const missing = checks.filter((check) => !check.met);
  if (missing.length === 0) {
    return (
      <span className="flex items-center gap-1.5 text-[0.75rem] text-emerald-600 dark:text-emerald-400">
        <CircleCheck size={13} aria-hidden="true" />
        Cluster has everything it needs
      </span>
    );
  }
  return (
    <div role="alert" className="flex flex-col gap-1 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2">
      <span className="flex items-center gap-1.5 text-[0.75rem] font-medium">
        <TriangleAlert size={13} className="text-amber-500" aria-hidden="true" />
        {`Cluster is missing: ${missing.map((check) => check.name).join(", ")}`}
      </span>
      <ul className="flex flex-col gap-0.5">
        {checks.map((check) => (
          <li key={check.name} className="flex items-start gap-1.5 text-[0.72rem] text-muted-foreground">
            {check.met ? (
              <Check size={12} className="mt-0.5 shrink-0 text-emerald-500" aria-label="Met" />
            ) : (
              <X size={12} className="mt-0.5 shrink-0 text-destructive" aria-label="Missing" />
            )}
            <span className="min-w-0 break-words">
              {check.name}
              {check.detail && ` - ${check.detail}`}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
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
  const { check, recheck } = useClusterCheck(config);
  const state = reachability(config, known, stale);
  const where = [config.context, config.namespace].filter(Boolean).join(" / ");
  const checkable = config.kind === "kubernetes" && Boolean(config.context);
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
            onClick={recheck}
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
    </div>
  );
}

function WorkspaceForm({
  editing,
  contexts,
  onChange,
  onSave,
  onCancel,
}: {
  editing: Editing;
  contexts: string[];
  onChange: (draft: WorkspaceDraft) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const { draft } = editing;
  const set = (key: keyof WorkspaceDraft) => (event: { target: { value: string } }) =>
    onChange({ ...draft, [key]: event.target.value });
  return (
    <div className="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
      <h3 className="text-[0.9rem] font-semibold">{editing.previous ? `Edit ${editing.previous}` : "New workspace"}</h3>
      <div className="grid grid-cols-2 gap-3">
        <div className={FIELD_CLASS}>
          <Label htmlFor="workspace-kind">Type</Label>
          <select
            id="workspace-kind"
            value={draft.kind}
            onChange={set("kind")}
            className="rounded-md border border-border-strong bg-card px-2.5 py-1.5 text-[0.85rem] text-foreground"
          >
            {WORKSPACE_KINDS.map((kind) => (
              <option key={kind.id} value={kind.id}>
                {kind.label}
              </option>
            ))}
          </select>
        </div>
        <div className={FIELD_CLASS}>
          <Label htmlFor="workspace-name">Name</Label>
          <Input
            id="workspace-name"
            autoFocus
            value={draft.name}
            onChange={set("name")}
            onKeyDown={advanceOnEnter("workspace-context")}
            placeholder="ubuntu-desktop"
          />
        </div>
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
      </div>
      <div className={FIELD_CLASS}>
        <Label htmlFor="workspace-image">Image</Label>
        <Input
          id="workspace-image"
          value={draft.image}
          onChange={set("image")}
          placeholder="ghcr.io/inference-gateway/cli:latest"
        />
        <span className={HINT_CLASS}>The image every session's pod runs. It needs the infer CLI.</span>
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

// The computers sessions can run on: the workspaces of ~/.infer/daemon.yaml.
// The daemon reads them when it starts, so a change asks for a CLI restart.
export function WorkspacesTab() {
  const { restartBackend, cleanMachines } = useDesktop();
  const [cleanArmed, setCleanArmed] = useState(false);
  const [configs, setConfigs] = useState<WorkspaceConfig[]>([]);
  const [known, setKnown] = useState<Record<string, Workspace>>({});
  const [contexts, setContexts] = useState<string[]>([]);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [problem, setProblem] = useState("");
  const [stale, setStale] = useState(false);
  const [notice, setNotice] = useState("");
  const [docker, setDocker] = useState(false);
  const [provisioning, setProvisioning] = useState(false);

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
    loadKnown(DAEMON_RETRIES);
  }, [loadKnown]);

  useEffect(() => {
    reload();
    api
      .kubeContexts()
      .then(setContexts)
      .catch(() => {});
    api
      .dockerAvailable()
      .then(setDocker)
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

  const detect = async () => {
    setNotice("");
    try {
      const added = await api.detectWorkspaces();
      if (added.length === 0) return setNotice("Every cluster in your kubeconfig already has a workspace.");
      changed();
      setNotice(`Added ${added.join(", ")}.`);
    } catch (e) {
      setProblem(String(e));
    }
  };

  const provision = async () => {
    setProblem("");
    setProvisioning(true);
    try {
      const name = await api.provisionLocalCluster(setNotice);
      changed();
      setNotice(`Provisioned ${name} on a local k3s cluster in Docker.`);
    } catch (e) {
      setNotice("");
      setProblem(String(e));
    } finally {
      setProvisioning(false);
    }
  };

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

  const restart = async () => {
    await restartBackend(false);
    setStale(false);
    reload();
  };

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">Workspaces</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        The compute your agents can run on. Give an agent a workspace in its editor and every new chat with it runs on a
        computer of its own there. A Kubernetes workspace needs the Inference Gateway operator and agent-sandbox in the
        cluster.
      </p>
      {stale && (
        <div className="mb-4 flex items-center gap-3 rounded-lg border border-amber-500/40 bg-amber-500/10 px-4 py-2.5 text-[0.8rem]">
          <span className="flex-1">Restart the CLI so the daemon loads your changes. Running chats are stopped.</span>
          <Button variant="outline" size="xs" onClick={restart}>
            Restart CLI now
          </Button>
        </div>
      )}
      {notice && (
        <p aria-live="polite" className="mb-4 text-[0.8rem] text-muted-foreground">
          {notice}
        </p>
      )}
      {problem && (
        <p role="alert" className="mb-4 text-[0.8rem] text-destructive">
          {problem}
        </p>
      )}
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
            contexts={contexts}
            onChange={(draft) => setEditing({ ...editing, draft })}
            onSave={save}
            onCancel={() => {
              setEditing(null);
              setProblem("");
            }}
          />
        ) : (
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setEditing({ previous: null, draft: draftOf(EMPTY_WORKSPACE) })}
            >
              New workspace
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={detect}
              title="Add a workspace for every cluster in your kubeconfig that has none yet"
            >
              Detect clusters
            </Button>
            {docker && (
              <Button
                variant="outline"
                size="sm"
                aria-label="Provision local cluster"
                title="Provision a local k3s cluster in Docker with an ubuntu-desktop workspace"
                disabled={provisioning}
                onClick={provision}
              >
                <Container size={14} className={provisioning ? "animate-pulse" : ""} />
              </Button>
            )}
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
              Detecting adds every kubeconfig context as a Kubernetes workspace in the infer-workspaces namespace, with
              the names of your saved API keys for its gateway.{" "}
              {docker
                ? "The container button runs a k3s cluster in Docker and adds an ubuntu-desktop workspace on it."
                : "Install and start Docker to provision a local cluster with one click."}
            </span>
          </div>
        )}
      </div>
    </>
  );
}
