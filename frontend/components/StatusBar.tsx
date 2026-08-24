import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { Zap } from "lucide-react";

const DOT: Record<string, string> = {
  error: "bg-destructive",
  awaiting: "animate-pulse bg-amber-500",
  running: "animate-pulse bg-primary",
  ready: "bg-emerald-500",
  stopped: "bg-red-500",
  idle: "bg-muted-foreground",
};

export function StatusBar() {
  const {
    statusText,
    statusError,
    running,
    ready,
    isAwaitingApproval,
    isRunning,
    runLabel,
    sessionId,
    conversations,
    openConversation,
    runningCount,
    autoMode,
    setAutoMode,
  } = useDesktop();
  const [open, setOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: MouseEvent) => {
      if (!dropdownRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  const agents = conversations
    .map((c) => ({ ...c, status: runLabel(c.id) }))
    .filter((c): c is typeof c & { status: { label: string; error: boolean } } => c.status != null)
    .map((c) => ({
      id: c.id,
      title: c.title || "Orchestrator",
      status: c.status,
      tone: c.status.error
        ? "error"
        : isAwaitingApproval(c.id)
          ? "awaiting"
          : isRunning(c.id)
            ? "running"
            : c.status.label === "Stopped"
              ? "stopped"
              : "ready",
    }));

  const session = !statusError && sessionId ? runLabel(sessionId) : null;
  const label = session?.label ?? statusText;
  const isError = session ? session.error : statusError;
  const tone = isError
    ? "error"
    : sessionId && isAwaitingApproval(sessionId)
      ? "awaiting"
      : running
        ? "running"
        : session?.label === "Stopped"
          ? "stopped"
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
      <div ref={dropdownRef} className="relative" role="status" aria-live="polite">
        {label && (
          <button
            aria-expanded={open}
            aria-haspopup="menu"
            aria-label="Agent status"
            onClick={() => setOpen((v) => !v)}
            className="flex items-center gap-[0.4rem] rounded px-1 py-0.5 hover:bg-secondary"
          >
            <span className={cn("h-[0.45rem] w-[0.45rem] shrink-0 rounded-full", DOT[tone])} />
            <span className={isError ? "text-err" : "text-muted-foreground"}>{label}</span>
            {runningCount > 0 && (
              <span
                title={`${runningCount} orchestrator${runningCount === 1 ? "" : "s"} running`}
                className="min-w-[1.1rem] rounded-full bg-primary/15 px-1 text-center text-[0.65rem] font-semibold text-primary"
              >
                {runningCount}
              </span>
            )}
          </button>
        )}
        {open && (
          <div
            role="menu"
            aria-label="Agents"
            className="absolute bottom-full left-0 z-50 mb-1 min-w-[18rem] max-w-[26rem] rounded-md border border-border bg-popover p-1 shadow-md"
          >
            {agents.length === 0 && (
              <div className="px-2 py-1.5 text-muted-foreground">No orchestrators running</div>
            )}
            {agents.map((a) => (
              <button
                key={a.id}
                role="menuitem"
                onClick={() => {
                  setOpen(false);
                  openConversation(a.id);
                }}
                className={cn(
                  "flex w-full items-center gap-[0.4rem] rounded px-2 py-1.5 text-left hover:bg-secondary",
                  a.id === sessionId && "bg-secondary/60"
                )}
              >
                <span className={cn("h-[0.45rem] w-[0.45rem] shrink-0 rounded-full", DOT[a.tone])} />
                <span className="truncate text-foreground">{a.title}</span>
                <span className="shrink-0 font-mono text-[0.68rem] text-muted-foreground/60">{a.id.slice(0, 5)}</span>
                <span
                  className={cn(
                    "ml-auto shrink-0 pl-3",
                    a.status.error ? "text-err" : "text-muted-foreground"
                  )}
                >
                  {a.status.label}
                </span>
              </button>
            ))}
          </div>
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
