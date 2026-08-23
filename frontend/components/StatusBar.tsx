import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { Zap } from "lucide-react";

const DOT: Record<string, string> = {
  error: "bg-destructive",
  awaiting: "animate-pulse bg-amber-500",
  running: "animate-pulse bg-primary",
  ready: "bg-emerald-500",
  idle: "bg-muted-foreground",
};

export function StatusBar() {
  const {
    statusText,
    statusError,
    running,
    ready,
    isAwaitingApproval,
    sessionId,
    autoMode,
    setAutoMode,
  } = useDesktop();
  const tone = statusError
    ? "error"
    : sessionId && isAwaitingApproval(sessionId)
      ? "awaiting"
      : running
        ? "running"
        : ready
          ? "ready"
          : "idle";
  const autoModeDescription = autoMode
    ? "Auto approval is on - new runs do not ask before tool actions"
    : "Auto approval is off - new runs ask before protected tool actions";
  return (
    <div
      id="status-bar"
      className="mx-auto mb-[0.45rem] flex max-w-[52rem] items-center gap-[0.4rem] px-1 text-[0.72rem] font-medium"
    >
      <div role="status" aria-live="polite" className="flex items-center gap-[0.4rem]">
        {statusText && (
          <>
            <span className={cn("h-[0.45rem] w-[0.45rem] shrink-0 rounded-full", DOT[tone])} />
            <span className={statusError ? "text-err" : "text-muted-foreground"}>{statusText}</span>
          </>
        )}
      </div>
      <button
        aria-label={autoModeDescription}
        aria-pressed={autoMode}
        title={autoModeDescription}
        onClick={() => setAutoMode(!autoMode)}
        className={cn(
          "ml-auto inline-flex size-7 items-center justify-center rounded-full",
          autoMode
            ? "bg-primary text-primary-foreground hover:bg-primary-hover"
            : "text-muted-foreground hover:bg-secondary hover:text-foreground",
        )}
      >
        <Zap size={16} aria-hidden="true" />
      </button>
    </div>
  );
}
