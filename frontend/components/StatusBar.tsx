import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";

const DOT: Record<string, string> = {
  error: "bg-destructive",
  awaiting: "animate-pulse bg-amber-500",
  running: "animate-pulse bg-primary",
  ready: "bg-emerald-500",
  idle: "bg-muted-foreground",
};

export function StatusBar() {
  const { statusText, statusError, running, ready, isAwaitingApproval, sessionId } = useDesktop();
  if (!statusText) return null;
  const tone = statusError
    ? "error"
    : sessionId && isAwaitingApproval(sessionId)
      ? "awaiting"
      : running
        ? "running"
        : ready
          ? "ready"
          : "idle";
  return (
    <div
      id="status-bar"
      role="status"
      aria-live="polite"
      className="mx-auto mb-[0.45rem] flex max-w-[52rem] items-center gap-[0.4rem] px-1 text-[0.72rem] font-medium"
    >
      <span className={cn("h-[0.45rem] w-[0.45rem] shrink-0 rounded-full", DOT[tone])} />
      <span className={statusError ? "text-err" : "text-muted-foreground"}>{statusText}</span>
    </div>
  );
}
