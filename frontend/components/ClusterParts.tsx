import { useCallback, useEffect, useState, type KeyboardEvent } from "react";
import { Check, CircleCheck, TriangleAlert, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { ClusterCheck, Prerequisite } from "@/lib/tauri";
import { useBackend } from "@/stores/backend";

export const FIELD_CLASS = "flex flex-col gap-1";
export const HINT_CLASS = "text-[0.72rem] text-muted-foreground";

export function advanceOnEnter(nextId: string) {
  return (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    document.getElementById(nextId)?.focus();
  };
}

/** A cluster check run when the row shows and again on demand. A null `run` checks nothing. */
export function useCheck(run: (() => Promise<ClusterCheck>) | null): {
  check: ClusterCheck | null;
  recheck: () => void;
} {
  const [check, setCheck] = useState<ClusterCheck | null>(null);
  const recheck = useCallback(() => {
    if (!run) return;
    setCheck(null);
    run()
      .then(setCheck)
      .catch((e) =>
        setCheck({
          prerequisites: [{ name: "Cluster reachable", met: false, detail: String(e) }],
          running: null,
          suspended: null,
        }),
      );
  }, [run]);
  useEffect(recheck, [recheck]);
  return { check, recheck };
}

export function RunningBadge({ check }: { check: ClusterCheck | null }) {
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

export function Prerequisites({ checks }: { checks: Prerequisite[] | null }) {
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

/** The CLI restart a daemon.yaml or compute.yaml change needs before the daemon loads it. */
export function RestartNotice({ onRestart }: { onRestart: () => void }) {
  return (
    <div className="mb-4 flex items-center gap-3 rounded-lg border border-amber-500/40 bg-amber-500/10 px-4 py-2.5 text-[0.8rem]">
      <span className="flex-1">Restart the CLI so the daemon loads your changes. Running chats are stopped.</span>
      <Button variant="outline" size="xs" onClick={onRestart}>
        Restart CLI now
      </Button>
    </div>
  );
}

/** The restart a settings tab offers after a change the daemon only loads on
 * start: `stale` shows the notice, and restarting reloads the tab's list. */
export function useRestart(reload: () => void) {
  const { restartBackend } = useBackend();
  const [stale, setStale] = useState(false);
  const restart = async () => {
    await restartBackend(false);
    setStale(false);
    reload();
  };
  return { stale, setStale, restart };
}

/** Why the tab's last action failed, announced to screen readers. */
export function Problem({ text }: { text: string }) {
  if (!text) return null;
  return (
    <p role="alert" className="mb-4 text-[0.8rem] text-destructive">
      {text}
    </p>
  );
}
