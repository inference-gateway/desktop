import { useCallback, useEffect, useMemo, useState } from "react";
import { Container, Pencil, RefreshCw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, type ClusterConfig, type ClusterRuntime, type LocalCluster } from "@/lib/tauri";
import { LOCAL_CLUSTER } from "@/lib/workspace-form";
import { clusterConfigOf, clusterDraftOf, clusterProblem, EMPTY_CLUSTER, type ClusterDraft } from "@/lib/cluster-form";
import { ClusterFolders } from "./ClusterFolders";
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

type Editing = { previous: string | null; draft: ClusterDraft };

const RUNTIME_LABEL: Record<ClusterRuntime, string> = { docker: "Docker", podman: "Podman" };
const RUNTIME_HINT: Record<ClusterRuntime, string> = {
  docker: "Its computers render on the CPU.",
  podman: "On a libkrun machine, so its computers get this Mac's GPU.",
};

function ClusterRow({
  config,
  runtime,
  onEdit,
  onDelete,
}: {
  config: ClusterConfig;
  runtime: ClusterRuntime | null;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const [armed, setArmed] = useState(false);
  const run = useMemo(() => () => api.checkCluster(config.name), [config.name]);
  const { check, recheck } = useCheck(run);
  return (
    <div className="flex flex-col gap-2 rounded-lg border border-border bg-card px-4 py-3">
      <div className="flex items-center gap-3">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-[0.88rem] font-medium">
            {config.name}
            <span className="ml-2 text-[0.72rem] font-normal text-muted-foreground">
              {runtime
                ? `Kubernetes on ${RUNTIME_LABEL[runtime]}${runtime === "podman" ? " with GPU" : ""}`
                : "Kubernetes"}
            </span>
            <RunningBadge check={check} />
          </span>
          <span className="truncate text-[0.75rem] text-muted-foreground">
            {[config.context || "current context", config.namespace].join(" / ")}
          </span>
        </div>
        <Button
          variant="outline"
          size="xs"
          aria-label={`Check cluster ${config.name}`}
          title="Check the cluster's prerequisites again"
          onClick={recheck}
        >
          <RefreshCw size={12} />
        </Button>
        <Button variant="outline" size="xs" aria-label={`Edit cluster ${config.name}`} onClick={onEdit}>
          <Pencil size={12} />
        </Button>
        <Button
          variant="outline"
          size="xs"
          aria-label={`Delete cluster ${config.name}`}
          title={armed ? "Click again to delete" : "Delete cluster"}
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
      <Prerequisites checks={check?.prerequisites ?? null} />
    </div>
  );
}

function ClusterForm({
  editing,
  contexts,
  onChange,
  onSave,
  onCancel,
}: {
  editing: Editing;
  contexts: string[];
  onChange: (draft: ClusterDraft) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const { draft } = editing;
  const set = (key: keyof ClusterDraft) => (event: { target: { value: string } }) =>
    onChange({ ...draft, [key]: event.target.value });
  return (
    <div className="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
      <h3 className="text-[0.9rem] font-semibold">{editing.previous ? `Edit ${editing.previous}` : "New cluster"}</h3>
      <div className="grid grid-cols-3 gap-3">
        <div className={FIELD_CLASS}>
          <Label htmlFor="cluster-name">Name</Label>
          <Input
            id="cluster-name"
            autoFocus
            value={draft.name}
            onChange={set("name")}
            onKeyDown={advanceOnEnter("cluster-context")}
            placeholder="k3d-agents"
          />
        </div>
        <div className={FIELD_CLASS}>
          <Label htmlFor="cluster-context">Kubernetes context</Label>
          <Input
            id="cluster-context"
            list="cluster-contexts"
            value={draft.context}
            onChange={set("context")}
            onKeyDown={advanceOnEnter("cluster-namespace")}
            placeholder="k3d-agents"
          />
          <datalist id="cluster-contexts">
            {contexts.map((context) => (
              <option key={context} value={context} />
            ))}
          </datalist>
        </div>
        <div className={FIELD_CLASS}>
          <Label htmlFor="cluster-namespace">Namespace</Label>
          <Input
            id="cluster-namespace"
            value={draft.namespace}
            onChange={set("namespace")}
            onKeyDown={advanceOnEnter("cluster-gateway-env")}
            placeholder="infer-workspaces"
          />
        </div>
      </div>
      <div className={FIELD_CLASS}>
        <Label htmlFor="cluster-gateway-env">Provider keys</Label>
        <Input
          id="cluster-gateway-env"
          value={draft.gateway_env}
          onChange={set("gateway_env")}
          placeholder="OLLAMA_CLOUD_API_KEY"
        />
        <span className={HINT_CLASS}>
          Names only, separated by commas. The values come from API Keys and go to the cluster's one gateway, which
          every workspace on it shares. They never enter an agent's pod.
        </span>
      </div>
      <div className="flex gap-2">
        <Button size="sm" onClick={onSave}>
          Save cluster
        </Button>
        <Button variant="outline" size="sm" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

// The clusters workspaces run on: ~/.infer/compute.yaml. The daemon reads it when it starts, so a change asks for a
// CLI restart.
export function ComputeTab() {
  const [clusters, setClusters] = useState<ClusterConfig[]>([]);
  const [contexts, setContexts] = useState<string[]>([]);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [problem, setProblem] = useState("");
  const [notice, setNotice] = useState("");
  const [supported, setSupported] = useState(true);
  const [local, setLocal] = useState<LocalCluster | null>(null);
  const [provisioning, setProvisioning] = useState(false);

  const reloadLocal = useCallback(() => {
    api
      .localClusterStatus()
      .then(setLocal)
      .catch(() => {});
  }, []);

  const reload = useCallback(() => {
    api
      .clusterConfigs()
      .then(setClusters)
      .catch((e) => setProblem(String(e)));
  }, []);
  const { stale, setStale, restart } = useRestart(reload);

  useEffect(() => {
    reload();
    api
      .kubeContexts()
      .then(setContexts)
      .catch(() => {});
    api
      .computeSupported()
      .then(setSupported)
      .catch(() => setSupported(false));
    reloadLocal();
  }, [reload, reloadLocal]);

  const changed = () => {
    setProblem("");
    setStale(true);
    reload();
  };

  const save = async () => {
    if (!editing) return;
    const blocked = clusterProblem(editing.draft);
    if (blocked) return setProblem(blocked);
    try {
      await api.saveCluster(clusterConfigOf(editing.draft), editing.previous ?? undefined);
      setEditing(null);
      changed();
    } catch (e) {
      setProblem(String(e));
    }
  };

  const remove = (name: string) =>
    api
      .deleteCluster(name)
      .then(changed)
      .catch((e) => setProblem(String(e)));

  const detect = async () => {
    setNotice("");
    try {
      const added = await api.detectClusters();
      if (added.length === 0)
        return setNotice("Every cluster in your kubeconfig is already here or used by a workspace.");
      changed();
      setNotice(`Added ${added.join(", ")}.`);
    } catch (e) {
      setProblem(String(e));
    }
  };

  const provision = async (recreate = false) => {
    setProblem("");
    setProvisioning(true);
    try {
      const { workspace, unrestored } = await api.provisionLocalCluster(setNotice, recreate);
      changed();
      setNotice(`Provisioned a local k3s cluster in ${runtimeLabel} with the ${workspace} workspace on it.`);
      if (unrestored.length > 0)
        setProblem(
          `The cluster runs, but these secrets did not reach it - save them again under Secrets: ${unrestored.join(". ")}`,
        );
    } catch (e) {
      setNotice("");
      setProblem(String(e));
    } finally {
      setProvisioning(false);
      reloadLocal();
    }
  };

  const choose = (runtime: ClusterRuntime) =>
    api
      .saveLocalClusterRuntime(runtime)
      .then(reloadLocal)
      .catch((e) => setProblem(String(e)));

  const runtimeLabel = RUNTIME_LABEL[local?.runtime ?? "docker"];
  const ready = local !== null && !local.problem;

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">Compute</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        The clusters your workspaces run on. A cluster is configured once here, and every workspace on it shares its
        namespace and its gateway. It needs the Inference Gateway operator and agent-sandbox.
      </p>
      {!supported && (
        <p
          role="alert"
          className="mb-4 rounded-lg border border-amber-500/40 bg-amber-500/10 px-4 py-2.5 text-[0.8rem]"
        >
          Clusters need an infer CLI with compute support. Install updates under Settings &gt; Updates. Workspaces that
          carry their own context keep working meanwhile.
        </p>
      )}
      {stale && <RestartNotice onRestart={restart} />}
      {notice && (
        <p aria-live="polite" className="mb-4 text-[0.8rem] text-muted-foreground">
          {notice}
        </p>
      )}
      <Problem text={problem} />
      <div className="flex flex-col gap-3">
        {clusters.length === 0 && !editing && <p className="text-[0.8rem] text-muted-foreground">No cluster yet.</p>}
        {clusters.map((config) => (
          <ClusterRow
            key={config.name}
            config={config}
            runtime={config.name === LOCAL_CLUSTER ? (local?.running ?? null) : null}
            onEdit={() => setEditing({ previous: config.name, draft: clusterDraftOf(config) })}
            onDelete={() => remove(config.name)}
          />
        ))}
        {editing ? (
          <ClusterForm
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
              disabled={!supported}
              onClick={() => setEditing({ previous: null, draft: clusterDraftOf(EMPTY_CLUSTER) })}
            >
              New cluster
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={!supported}
              onClick={detect}
              title="Add a cluster for every context in your kubeconfig that nothing uses yet"
            >
              Detect clusters
            </Button>
            {ready && (
              <Button
                variant="outline"
                size="sm"
                aria-label="Provision local cluster"
                title={`Provision a local k3s cluster in ${runtimeLabel} with an ubuntu-desktop workspace on it`}
                disabled={provisioning || !supported}
                onClick={() => provision()}
              >
                <Container size={14} className={provisioning ? "animate-pulse" : ""} />
              </Button>
            )}
            <span className={HINT_CLASS}>
              Detecting adds every kubeconfig context as a cluster in the infer-workspaces namespace, with the names of
              your saved API keys for its gateway.{" "}
              {ready
                ? `The container button runs a k3s cluster in ${runtimeLabel} and adds an ubuntu-desktop workspace on it.`
                : local?.runtime !== "podman" && local?.problem}
            </span>
          </div>
        )}
      </div>
      {local && local.runtimes.length > 1 && (
        <section className="mt-6 flex flex-col gap-2">
          <h3 className="text-[0.9rem] font-semibold">Local cluster runtime</h3>
          <div className="flex items-center gap-2">
            {local.runtimes.map((runtime) => (
              <Button
                key={runtime}
                variant={runtime === local.runtime ? "default" : "outline"}
                size="xs"
                aria-label={`Run local cluster on ${RUNTIME_LABEL[runtime]}`}
                aria-pressed={runtime === local.runtime}
                title={RUNTIME_HINT[runtime]}
                disabled={provisioning}
                onClick={() => choose(runtime)}
              >
                {RUNTIME_LABEL[runtime]}
              </Button>
            ))}
            <span className={HINT_CLASS}>
              {`${RUNTIME_HINT[local.runtime]} `}
              {local.running ? `The cluster runs on ${RUNTIME_LABEL[local.running]}.` : "No local cluster runs yet."}
            </span>
          </div>
          {local.runtime === "podman" && <Problem text={local.problem ?? ""} />}
        </section>
      )}
      {local && (ready || local.pending) && supported && (
        <ClusterFolders
          busy={provisioning}
          pending={local.pending}
          onSaved={reloadLocal}
          onRecreate={() => provision(true)}
        />
      )}
    </>
  );
}
