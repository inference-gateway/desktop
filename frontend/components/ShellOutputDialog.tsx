import { useEffect, useState } from "react";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api, type BackgroundJob } from "@/lib/tauri";

const REFRESH_MS = 2000;

export type ViewedShell = { sessionId: string; project?: string; jobId: string };

/** The job as the session's worker reports it, re-read while it is being looked at. */
function useBackgroundJob(shell: ViewedShell | null): { job: BackgroundJob | null; error: string } {
  const [job, setJob] = useState<BackgroundJob | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    setJob(null);
    setError("");
    if (!shell) return;
    let cancelled = false;
    let pending = false;
    const read = async () => {
      if (pending) return;
      pending = true;
      try {
        const jobs = await api.listBackgroundJobs(shell.sessionId, shell.project);
        if (cancelled) return;
        const found = jobs.find((j) => j.id === shell.jobId);
        if (found) setJob(found);
        setError(found ? "" : "This shell is no longer tracked by the session.");
      } catch (e) {
        if (!cancelled) setError(String(e));
      } finally {
        pending = false;
      }
    };
    read();
    const timer = setInterval(read, REFRESH_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [shell]);
  return { job, error };
}

/** What a background shell of a chat is running and what it has printed so far. */
export function ShellOutputDialog({ shell, onClose }: { shell: ViewedShell | null; onClose: () => void }) {
  const { job, error } = useBackgroundJob(shell);
  return (
    <Dialog open={shell !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Background shell {shell?.jobId}</DialogTitle>
          <DialogDescription>{job ? `Status: ${job.status}` : error ? "" : "Loading..."}</DialogDescription>
        </DialogHeader>
        {job && (
          <>
            <code className="break-all rounded bg-secondary px-2 py-1 text-[0.78rem]">{job.detail}</code>
            <pre
              aria-label="Shell output"
              className="max-h-80 overflow-auto whitespace-pre-wrap break-all rounded bg-secondary p-2 text-[0.78rem]"
            >
              {job.output || "No output yet."}
            </pre>
          </>
        )}
        {error && (
          <p aria-live="polite" className="text-[0.8rem] text-muted-foreground">
            {error}
          </p>
        )}
      </DialogContent>
    </Dialog>
  );
}
